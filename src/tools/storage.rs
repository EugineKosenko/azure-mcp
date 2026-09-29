use crate::client;
use crate::tools;

struct Blob {
    name: String,
    size: u64,
    kind: String,
    tier: String,
    changed: String,
}

fn tag<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let start = text.find(&format!("<{}>", name))? + name.len() + 2;
    let end = text[start..].find(&format!("</{}>", name))?;

    Some(&text[start..start + end])
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn parse(text: &str) -> (Vec<Blob>, Option<String>) {
    let blobs = text
        .split("<Blob>")
        .skip(1)
        .map(|item| Blob {
            name: unescape(tag(item, "Name").unwrap()),
            size: tag(item, "Content-Length").unwrap().parse().unwrap(),
            kind: tag(item, "BlobType").unwrap_or("-").to_string(),
            tier: tag(item, "AccessTier").unwrap_or("-").to_string(),
            changed: tag(item, "Last-Modified").unwrap_or("-").to_string(),
        })
        .collect();

    (blobs, tag(text, "NextMarker").filter(|marker| !marker.is_empty()).map(str::to_string))
}
fn size(bytes: u64) -> String {
    match bytes {
        1_073_741_824.. => format!("{:.2} ГБ", bytes as f64 / 1_073_741_824.0),
        1_048_576.. => format!("{:.1} МБ", bytes as f64 / 1_048_576.0),
        _ => format!("{} Б", bytes),
    }
}
fn account(item: &serde_json::Value) -> String {
    format!(
        "{}  {}  {}  {}  {}  {}  created={}",
        tools::text(item, "/name"),
        tools::part(tools::text(item, "/id"), "resourceGroups").to_lowercase(),
        tools::text(item, "/location"),
        tools::text(item, "/sku/name"),
        tools::text(item, "/kind"),
        tools::text(item, "/properties/accessTier"),
        tools::text(item, "/properties/creationTime").chars().take(10).collect::<String>(),
    )
}

fn container(item: &serde_json::Value) -> String {
    format!(
        "  {}  public={}  changed={}",
        tools::text(item, "/name"),
        tools::text(item, "/properties/publicAccess"),
        tools::text(item, "/properties/lastModifiedTime"),
    )
}

fn blob(item: &Blob) -> String {
    format!("  {}  {}  {}  {}  {}", item.name, size(item.size), item.kind, item.tier, item.changed)
}

fn total(blobs: &[Blob]) -> String {
    format!("blobs={}  size={}", blobs.len(), size(blobs.iter().map(|item| item.size).sum()))
}
async fn all(http: &reqwest::Client, account: &str, container: &str) -> Result<Vec<Blob>, String> {
    let mut blobs = Vec::new();
    let mut marker: Option<String> = None;

    loop {
        let (mut found, next) = parse(&client::page(http, account, container, marker.as_deref()).await?);

        blobs.append(&mut found);

        match next {
            Some(next) => marker = Some(next),
            None => return Ok(blobs),
        }
    }
}

async fn detail(http: &reqwest::Client, item: &serde_json::Value, arguments: &serde_json::Value) -> Result<String, String> {
    let name = tools::text(item, "/name");
    let path = format!("{}/blobServices/default/containers?api-version=2023-05-01", tools::text(item, "/id"));
    let containers = client::list(http, &path).await?;
    let mut lines = vec![account(item)];

    for one in tools::sorted(&containers) {
        let mut line = container(one);

        if arguments["sizes"].as_bool().unwrap_or(false) {
            line = format!("{}  {}", line, total(&all(http, name, tools::text(one, "/name")).await?));
        }

        lines.push(line);
    }

    if let Some(one) = arguments["container"].as_str() {
        let blobs = all(http, name, one).await?;

        lines.push(format!("\nBlob-и контейнера {}: {}", one, total(&blobs)));
        lines.extend(blobs.iter().map(blob));
    }

    Ok(lines.join("\n"))
}

pub async fn run(http: &reqwest::Client, arguments: &serde_json::Value) -> serde_json::Value {
    let scope = match tools::scope(arguments).await {
        Ok(scope) => scope,
        Err(message) => return tools::reply(Err(message)),
    };
    let path = format!("{}/providers/Microsoft.Storage/storageAccounts?api-version=2023-05-01", scope);
    let accounts = match client::list(http, &path).await {
        Ok(accounts) => accounts,
        Err(message) => return tools::reply(Err(message)),
    };
    
    let Some(name) = arguments["account"].as_str() else {
        let lines: Vec<String> = tools::sorted(&accounts).into_iter().map(account).collect();
    
        return tools::reply(Ok(if lines.is_empty() { "Облікових записів не знайдено.".to_string() } else { lines.join("\n") }));
    };
    
    match accounts.iter().find(|item| tools::same(tools::text(item, "/name"), name)) {
        Some(item) => tools::reply(detail(http, item, arguments).await),
        None => tools::reply(Err(format!("Облікового запису {} не знайдено в цій області.", name))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?><EnumerationResults ContainerName=\"vhds\"><Blobs>\
        <Blob><Name>debian&amp;base.vhd</Name><Properties><Last-Modified>Mon, 01 Sep 2026 10:00:00 GMT</Last-Modified>\
        <Content-Length>4294967296</Content-Length><BlobType>PageBlob</BlobType><AccessTier>Hot</AccessTier></Properties></Blob>\
        <Blob><Name>notes.txt</Name><Properties><Last-Modified>Tue, 02 Sep 2026 11:00:00 GMT</Last-Modified>\
        <Content-Length>512</Content-Length><BlobType>BlockBlob</BlobType></Properties></Blob>\
        </Blobs><NextMarker>next-page</NextMarker></EnumerationResults>";
    
    #[test]
    fn parse_reads_blobs_and_marker() {
        let (blobs, marker) = parse(XML);
    
        assert_eq!(blobs.len(), 2);
        assert_eq!(blob(&blobs[0]), "  debian&base.vhd  4.00 ГБ  PageBlob  Hot  Mon, 01 Sep 2026 10:00:00 GMT");
        assert_eq!(blob(&blobs[1]), "  notes.txt  512 Б  BlockBlob  -  Tue, 02 Sep 2026 11:00:00 GMT");
        assert_eq!(marker, Some("next-page".to_string()));
    }
    
    #[test]
    fn parse_last_page_has_no_marker() {
        let (blobs, marker) = parse("<EnumerationResults><Blobs></Blobs><NextMarker /></EnumerationResults>");
    
        assert!(blobs.is_empty());
        assert_eq!(marker, None);
    }
    
    #[test]
    fn total_sums_sizes() {
        assert_eq!(total(&parse(XML).0), "blobs=2  size=4.00 ГБ");
    }
    
    #[test]
    fn size_picks_unit() {
        assert_eq!(size(512), "512 Б");
        assert_eq!(size(1_572_864), "1.5 МБ");
        assert_eq!(size(4_294_967_296), "4.00 ГБ");
    }
    
    #[test]
    fn account_and_container_lines() {
        let item = serde_json::json!({
            "name": "demostore",
            "id": "/subscriptions/s/resourceGroups/DEMO-RG/providers/Microsoft.Storage/storageAccounts/demostore",
            "location": "eastus",
            "kind": "StorageV2",
            "sku": { "name": "Standard_LRS" },
            "properties": { "accessTier": "Hot", "creationTime": "2026-08-01T09:30:00.0000000Z" },
        });
        let one = serde_json::json!({ "name": "vhds", "properties": { "publicAccess": "None", "lastModifiedTime": "2026-09-01T10:00:00Z" } });
    
        assert_eq!(account(&item), "demostore  demo-rg  eastus  Standard_LRS  StorageV2  Hot  created=2026-08-01");
        assert_eq!(container(&one), "  vhds  public=None  changed=2026-09-01T10:00:00Z");
    }
}
