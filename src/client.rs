use tokio::process::Command;
use std::env;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::path::{Path, PathBuf};
use reqwest::Method;

const ARM: &str = "https://management.azure.com";
const LOGIN: &str = "Якщо це AADSTS50078 (сплив термін багатофакторної автентифікації), користувач має виконати інтерактивно: az login --scope https://management.core.windows.net//.default (за потреби з --tenant <тенант>).";

async fn az(args: &[&str]) -> Result<String, String> {
    let output = Command::new("az")
        .args(args)
        .output()
        .await
        .map_err(|error| format!("Не вдалося запустити az (він має бути в PATH): {}", error))?;

    if !output.status.success() {
        return Err(format!("az завершився з помилкою: {}\n{}", String::from_utf8_lossy(&output.stderr).trim(), LOGIN));
    }

    Ok(String::from_utf8(output.stdout).unwrap().trim().to_string())
}
pub async fn sbscrptn() -> Result<String, String> {
    match env::var("AZURE_SBSCRPTN") {
        Ok(id) if !id.is_empty() => Ok(id),
        _ => az(&["account", "show", "--query", "id", "-o", "tsv"]).await,
    }
}
struct Token {
    value: String,
    until: Instant,
}

static TOKEN: Mutex<Option<Token>> = Mutex::new(None);

fn saved() -> Option<String> {
    let guard = TOKEN.lock().unwrap();

    guard.as_ref().filter(|token| token.until > Instant::now()).map(|token| token.value.clone())
}

fn save(value: &str, epoch: u64) {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let left = epoch.saturating_sub(now).saturating_sub(60);

    *TOKEN.lock().unwrap() = Some(Token { value: value.to_string(), until: Instant::now() + Duration::from_secs(left) });
}

async fn token() -> Result<String, String> {
    if let Some(value) = saved() {
        return Ok(value);
    }

    let text = az(&[
        "account", "get-access-token", "--resource", ARM,
        "--query", "{token: accessToken, until: expires_on}", "-o", "json",
    ]).await?;
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    let value = json["token"].as_str().unwrap();

    save(value, json["until"].as_u64().unwrap());

    Ok(value.to_string())
}
const LIFE: Duration = Duration::from_secs(3600);

fn digest(text: &str) -> u64 {
    text.bytes().fold(0xcbf29ce484222325, |hash, byte| (hash ^ byte as u64).wrapping_mul(0x100000001b3))
}

fn cache_path(key: &str) -> PathBuf {
    let dir = match env::var("AZURE_CACHE") {
        Ok(dir) => PathBuf::from(dir),
        Err(_) => env::temp_dir().join("azure-mcp"),
    };

    dir.join(format!("{:016x}.json", digest(key)))
}

fn cache_read(path: &Path) -> Option<(serde_json::Value, u64)> {
    let age = std::fs::metadata(path).ok()?.modified().ok()?.elapsed().ok()?;

    if age > LIFE {
        return None;
    }

    Some((serde_json::from_str(&std::fs::read_to_string(path).ok()?).unwrap(), age.as_secs() / 60))
}
const PAUSES: [u64; 3] = [5, 15, 30];
const LONGEST: u64 = 60;

fn told(response: &reqwest::Response) -> Option<u64> {
    response
        .headers()
        .iter()
        .filter(|(name, _)| name.as_str().ends_with("retry-after"))
        .filter_map(|(_, value)| value.to_str().ok()?.parse::<u64>().ok())
        .max()
}

fn limits(response: &reqwest::Response) -> String {
    response
        .headers()
        .iter()
        .filter(|(name, _)| name.as_str().contains("ratelimit") || name.as_str().contains("retry"))
        .map(|(name, value)| format!("{}: {}", name, value.to_str().unwrap()))
        .collect::<Vec<_>>()
        .join("; ")
}
fn hint(status: u16) -> &'static str {
    match status {
        401 => "Токен не прийнято. Новий вхід робить лише користувач, інтерактивно: az login --scope https://management.core.windows.net//.default (за потреби з --tenant <тенант>).",
        403 => "Недостатньо прав на цю область: для читання потрібна роль Reader (для costs — Cost Management Reader).",
        404 => "Перевірте назву групи ресурсів (az group list) і підписки (az account list): неіснуюча область дає 404.",
        _ => "",
    }
}

fn failure(status: reqwest::StatusCode, url: &str, text: &str) -> String {
    match hint(status.as_u16()) {
        "" => format!("Azure відповів {} для {}: {}", status, url, text),
        hint => format!("Azure відповів {} для {}: {}\n{}", status, url, text, hint),
    }
}

async fn send(http: &reqwest::Client, method: Method, url: &str, body: Option<&serde_json::Value>) -> Result<String, String> {
    let mut pauses = PAUSES.iter();

    loop {
        let mut request = http.request(method.clone(), url).bearer_auth(token().await?);

        if let Some(body) = body {
            request = request.header("content-type", "application/json").body(body.to_string());
        }

        let response = request
            .send()
            .await
            .map_err(|error| format!("Запит до Azure не вдався: {}", error))?;
        let status = response.status();

        if status.as_u16() != 429 {
            let text = response.text().await.unwrap();

            return if status.is_success() { Ok(text) } else { Err(failure(status, url, &text)) };
        }

        let seconds = told(&response);
        let seen = limits(&response);

        match pauses.next() {
            Some(&pause) if seconds.unwrap_or(pause) <= LONGEST => {
                tokio::time::sleep(Duration::from_secs(seconds.unwrap_or(pause))).await;
            }
            _ => return Err(format!("Ліміт Azure вичерпано (429) для {}: повторіть пізніше. Заголовки ліміту: {}.", url, seen)),
        }
    }
}
pub async fn query(http: &reqwest::Client, path: &str, body: &serde_json::Value, refresh: bool) -> Result<(serde_json::Value, Option<u64>), String> {
    let url = format!("{}{}", ARM, path);
    let cache = cache_path(&format!("{}\n{}", url, body));

    if !refresh {
        if let Some((value, age)) = cache_read(&cache) {
            return Ok((value, Some(age)));
        }
    }

    let text = send(http, Method::POST, &url, Some(body)).await?;

    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    std::fs::write(&cache, &text).unwrap();

    Ok((serde_json::from_str(&text).unwrap(), None))
}
pub async fn list(http: &reqwest::Client, path: &str) -> Result<Vec<serde_json::Value>, String> {
    let mut url = format!("{}{}", ARM, path);
    let mut items = Vec::new();

    loop {
        let body: serde_json::Value = serde_json::from_str(&send(http, Method::GET, &url, None).await?).unwrap();

        items.extend(body["value"].as_array().unwrap().iter().cloned());

        match body["nextLink"].as_str() {
            Some(next) => url = next.to_string(),
            None => return Ok(items),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_adds_hint() {
        let missing = failure(reqwest::StatusCode::NOT_FOUND, "https://x", "нема");
        assert!(missing.starts_with("Azure відповів 404 Not Found для https://x: нема\n"));
        assert!(missing.contains("az group list"));
    
        assert!(failure(reqwest::StatusCode::UNAUTHORIZED, "u", "b").contains("az login"));
        assert!(failure(reqwest::StatusCode::FORBIDDEN, "u", "b").contains("Reader"));
        assert_eq!(failure(reqwest::StatusCode::BAD_REQUEST, "u", "b"), "Azure відповів 400 Bad Request для u: b");
    }
    
    #[test]
    fn digest_is_stable() {
        assert_eq!(digest(""), 0xcbf29ce484222325);
        assert_eq!(digest("a"), 0xaf63dc4c8601ec8c);
        assert_ne!(digest("a"), digest("b"));
    }
}
