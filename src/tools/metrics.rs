use std::time::{SystemTime, UNIX_EPOCH};

use crate::client;
use crate::tools;

fn seconds(text: &str) -> Option<u64> {
    let unit = text.chars().last()?;
    let count: u64 = text[..text.len() - unit.len_utf8()].parse().ok()?;

    match unit {
        'm' => Some(count * 60),
        'h' => Some(count * 3600),
        'd' => Some(count * 86400),
        _ => None,
    }
}

fn iso(epoch: u64) -> String {
    let z = (epoch / 86400) as i64 + 719468;
    let rest = epoch % 86400;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };

    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", year, month, day, rest / 3600, rest % 3600 / 60, rest % 60)
}
fn values(metric: &serde_json::Value, aggregation: &str) -> Vec<(String, f64)> {
    let key = aggregation.to_lowercase();

    metric["timeseries"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|series| series["data"].as_array().unwrap().iter())
        .filter_map(|point| Some((tools::text(point, "/timeStamp").to_string(), point[&key].as_f64()?)))
        .collect()
}

fn summary(label: &str, aggregation: &str, points: &[(String, f64)], threshold: Option<f64>) -> String {
    if points.is_empty() {
        return format!("{}  {}: даних немає", label, aggregation);
    }

    let all: Vec<f64> = points.iter().map(|(_, value)| *value).collect();
    let mean = all.iter().sum::<f64>() / all.len() as f64;
    let mut line = format!(
        "{}  {}: avg={:.2}  max={:.2}  min={:.2}  intervals={}",
        label,
        aggregation,
        mean,
        all.iter().cloned().fold(f64::MIN, f64::max),
        all.iter().cloned().fold(f64::MAX, f64::min),
        all.len(),
    );

    if let Some(limit) = threshold {
        line = format!("{}  above{}={}", line, limit, all.iter().filter(|value| **value > limit).count());
    }

    line
}

fn lines(metric: &serde_json::Value, aggregations: &[&str], threshold: Option<f64>, series: bool) -> Vec<String> {
    let label = format!("{} ({})", tools::text(metric, "/name/value"), tools::text(metric, "/unit"));
    let mut lines = Vec::new();

    for aggregation in aggregations {
        let points = values(metric, aggregation);

        lines.push(summary(&label, aggregation, &points, threshold));

        if series {
            lines.extend(points.iter().map(|(time, value)| format!("  {}  {:.2}", time.chars().take(16).collect::<String>(), value)));
        }
    }

    lines
}
fn definition(item: &serde_json::Value) -> String {
    let supported: Vec<&str> = item["supportedAggregationTypes"].as_array().map_or(Vec::new(), |items| items.iter().map(|kind| kind.as_str().unwrap()).collect());

    format!(
        "{}  {}  primary={}  aggregations={}",
        tools::text(item, "/name/value"),
        tools::text(item, "/unit"),
        tools::text(item, "/primaryAggregationType"),
        if supported.is_empty() { "-".to_string() } else { supported.join(",") },
    )
}
async fn locate(http: &reqwest::Client, arguments: &serde_json::Value) -> Result<String, String> {
    if let Some(id) = arguments["resource"].as_str() {
        return Ok(id.to_string());
    }

    let name = arguments["name"].as_str().ok_or("Вкажіть resource (повний ідентифікатор) чи name (ім'я ресурсу).".to_string())?;
    let scope = tools::scope(arguments).await?;
    let kind = arguments["type"].as_str().map_or(String::new(), |kind| format!(" and resourceType eq '{}'", kind));
    let path = format!("{}/resources?$filter=name eq '{}'{}&api-version=2021-04-01", scope, name.replace('\'', "''"), kind);
    let found = client::list(http, &path).await?;

    match found.as_slice() {
        [one] => Ok(tools::text(one, "/id").to_string()),
        [] => Err(format!("Ресурс {} не знайдено в цій області.", name)),
        many => Err(format!(
            "Ресурсів з ім'ям {} кілька, вкажіть resource (повний ідентифікатор), group чи type:\n{}",
            name,
            tools::sorted(many).iter().map(|one| format!("{}  {}  {}", tools::part(tools::text(one, "/id"), "resourceGroups").to_lowercase(), tools::text(one, "/type"), tools::text(one, "/name"))).collect::<Vec<_>>().join("\n"),
        )),
    }
}
async fn report(http: &reqwest::Client, arguments: &serde_json::Value) -> Result<String, String> {
    tools::unknown(arguments, &["resource", "name", "type", "list", "metric", "timespan", "start", "end", "interval", "aggregation", "threshold", "points", "group", "sbscrptn"])?;

    let id = locate(http, arguments).await?;

    if arguments["list"].as_bool().unwrap_or(false) {
        let path = format!("{}/providers/microsoft.insights/metricDefinitions?api-version=2018-01-01", id);
        let definitions = client::list(http, &path).await?;

        return Ok(definitions.iter().map(definition).collect::<Vec<_>>().join("\n"));
    }

    let names = arguments["metric"].as_str().ok_or("Вкажіть metric (ім'я метрики, кілька через кому) чи list=true для переліку доступних.".to_string())?;
    let interval = arguments["interval"].as_str().unwrap_or("PT1H");
    let aggregation = arguments["aggregation"].as_str().unwrap_or("Average");
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let (start, end) = match arguments["start"].as_str() {
        Some(start) => (start.to_string(), arguments["end"].as_str().map_or(iso(now), str::to_string)),
        None => {
            let span = arguments["timespan"].as_str().unwrap_or("24h");
            let back = seconds(span).ok_or(format!("Некоректний timespan {}: очікується число й одиниця m, h чи d, напр. 30m, 7d.", span))?;

            (iso(now - back), iso(now))
        }
    };
    let path = format!(
        "{}/providers/microsoft.insights/metrics?api-version=2023-10-01&metricnames={}&timespan={}/{}&interval={}&aggregation={}",
        id, names, start, end, interval, aggregation,
    );
    let body = client::get(http, &path).await?;
    let aggregations: Vec<&str> = aggregation.split(',').collect();
    let mut result = vec![format!("{}\n{} — {}  interval={}", id, start, end, interval)];

    for metric in body["value"].as_array().unwrap() {
        result.extend(lines(metric, &aggregations, arguments["threshold"].as_f64(), arguments["points"].as_bool().unwrap_or(false)));
    }

    Ok(result.join("\n"))
}

pub async fn run(http: &reqwest::Client, arguments: &serde_json::Value) -> serde_json::Value {
    tools::reply(report(http, arguments).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metric() -> serde_json::Value {
        serde_json::json!({
            "name": { "value": "Percentage CPU", "localizedValue": "Percentage CPU" },
            "unit": "Percent",
            "timeseries": [{ "data": [
                { "timeStamp": "2026-09-28T10:00:00Z", "average": 10.0, "maximum": 20.0 },
                { "timeStamp": "2026-09-28T11:00:00Z", "average": 50.0, "maximum": 70.0 },
                { "timeStamp": "2026-09-28T12:00:00Z", "average": 40.0, "maximum": 45.0 },
                { "timeStamp": "2026-09-28T13:00:00Z" },
            ] }],
        })
    }
    
    #[test]
    fn summary_counts_above_threshold() {
        assert_eq!(
            lines(&metric(), &["Average"], Some(40.0), false),
            vec!["Percentage CPU (Percent)  Average: avg=33.33  max=50.00  min=10.00  intervals=3  above40=1".to_string()]
        );
    }
    
    #[test]
    fn points_and_several_aggregations() {
        let result = lines(&metric(), &["Average", "Maximum"], None, true);
    
        assert_eq!(result.len(), 8);
        assert_eq!(result[1], "  2026-09-28T10:00  10.00");
        assert!(result[4].starts_with("Percentage CPU (Percent)  Maximum: avg=45.00  max=70.00  min=20.00"));
    }
    
    #[test]
    fn summary_without_data() {
        assert_eq!(lines(&metric(), &["Total"], None, false), vec!["Percentage CPU (Percent)  Total: даних немає".to_string()]);
    }
    
    #[test]
    fn timespan_units() {
        assert_eq!(seconds("30m"), Some(1800));
        assert_eq!(seconds("24h"), Some(86400));
        assert_eq!(seconds("7d"), Some(604800));
        assert_eq!(seconds("7"), None);
        assert_eq!(seconds("xd"), None);
    }
    
    #[test]
    fn iso_from_epoch() {
        assert_eq!(iso(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(iso(1_709_164_800), "2024-02-29T00:00:00Z");
    }
    
    #[test]
    fn definition_line() {
        let item = serde_json::json!({
            "name": { "value": "Percentage CPU" },
            "unit": "Percent",
            "primaryAggregationType": "Average",
            "supportedAggregationTypes": ["None", "Average", "Minimum", "Maximum"],
        });
    
        assert_eq!(definition(&item), "Percentage CPU  Percent  primary=Average  aggregations=None,Average,Minimum,Maximum");
        assert_eq!(definition(&serde_json::json!({})), "-  -  primary=-  aggregations=-");
    }
}
