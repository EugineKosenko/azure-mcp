use crate::tools;
use std::collections::BTreeMap;
use crate::client;

const PERIODS: [&str; 5] = ["MonthToDate", "BillingMonthToDate", "TheLastMonth", "TheLastBillingMonth", "WeekToDate"];
const SPLITS: [&str; 5] = ["ResourceGroupName", "ResourceId", "ResourceType", "MeterCategory", "ServiceName"];
fn date_ok(text: &str) -> bool {
    text.len() == 10 && text.bytes().enumerate().all(|(index, byte)| if index == 4 || index == 7 { byte == b'-' } else { byte.is_ascii_digit() })
}
fn request(period: &str, span: Option<(&str, &str)>, split: &str, daily: bool) -> serde_json::Value {
    let mut body = serde_json::json!({
        "type": "ActualCost",
        "timeframe": if span.is_some() { "Custom" } else { period },
        "dataset": {
            "granularity": if daily { "Daily" } else { "None" },
            "aggregation": { "totalCost": { "name": "Cost", "function": "Sum" } },
            "grouping": [{ "type": "Dimension", "name": split }],
        },
    });

    if let Some((from, to)) = span {
        body["timePeriod"] = serde_json::json!({
            "from": format!("{}T00:00:00+00:00", from),
            "to": format!("{}T23:59:59+00:00", to),
        });
    }

    body
}
fn short(id: &str) -> String {
    let Some((head, rest)) = id.split_once("/providers/") else { return id.to_string() };
    let items: Vec<&str> = rest.split('/').collect();

    if items.len() < 3 {
        return id.to_string();
    }

    let kinds: Vec<&str> = items[1..].iter().step_by(2).copied().collect();

    format!("{}  {}  {}", tools::part(head, "resourcegroups"), kinds.join("/"), items.last().unwrap())
}

fn label(name: &str, split: &str) -> String {
    if split == "ResourceId" { short(name) } else { name.to_string() }
}
fn column(body: &serde_json::Value, name: &str) -> usize {
    body["properties"]["columns"].as_array().unwrap().iter().position(|column| column["name"] == name).unwrap()
}

fn table(body: &serde_json::Value, split: &str) -> String {
    let rows = body["properties"]["rows"].as_array().unwrap();

    if rows.is_empty() {
        return "Витрат за цей період немає.".to_string();
    }

    let (cost, name, currency) = (column(body, "Cost"), column(body, split), column(body, "Currency"));
    let mut items: Vec<(f64, String)> = rows.iter().map(|row| (row[cost].as_f64().unwrap(), label(row[name].as_str().unwrap(), split))).collect();
    items.sort_by(|left, right| right.0.partial_cmp(&left.0).unwrap());

    let total: f64 = items.iter().map(|item| item.0).sum();
    let mut lines: Vec<String> = items.iter().map(|(amount, name)| format!("{:>10.2}  {}", amount, name)).collect();
    lines.push(format!("{:>10.2}  РАЗОМ, {}", total, rows[0][currency].as_str().unwrap()));

    if !body["properties"]["nextLink"].is_null() {
        lines.push("Є ще сторінки відповіді (nextLink), вони не показані.".to_string());
    }

    lines.join("\n")
}
fn stamp(value: &serde_json::Value) -> String {
    let text = value.to_string().trim_matches('"').to_string();

    if text.len() == 8 && text.bytes().all(|byte| byte.is_ascii_digit()) {
        format!("{}-{}-{}", &text[..4], &text[4..6], &text[6..])
    } else {
        text.chars().take(10).collect()
    }
}

fn average(sum: f64, full: f64) -> String {
    if full == 0.0 { format!("{:>10}", "-") } else { format!("{:>10.2}", sum / full) }
}

fn series(body: &serde_json::Value, split: &str) -> String {
    let rows = body["properties"]["rows"].as_array().unwrap();

    if rows.is_empty() {
        return "Витрат за цей період немає.".to_string();
    }

    let (cost, date, name, currency) = (column(body, "Cost"), column(body, "UsageDate"), column(body, split), column(body, "Currency"));
    let mut days: BTreeMap<String, f64> = BTreeMap::new();
    let mut items: Vec<(String, String, f64)> = Vec::new();

    for row in rows {
        let (day, amount) = (stamp(&row[date]), row[cost].as_f64().unwrap());

        *days.entry(day.clone()).or_insert(0.0) += amount;
        items.push((day, label(row[name].as_str().unwrap(), split), amount));
    }

    let last = days.keys().last().unwrap().clone();
    let full = (days.len() - 1) as f64;

    let mut sums: BTreeMap<String, (f64, f64)> = BTreeMap::new();
    for (day, name, amount) in &items {
        let entry = sums.entry(name.clone()).or_insert((0.0, 0.0));

        entry.0 += amount;
        if *day != last {
            entry.1 += amount;
        }
    }

    let mut groups: Vec<(String, f64, f64)> = sums.into_iter().map(|(name, (sum, part))| (name, sum, part)).collect();
    groups.sort_by(|left, right| right.1.partial_cmp(&left.1).unwrap());

    let mut lines = vec![format!("Щодоби, {}:", rows[0][currency].as_str().unwrap())];
    for (day, amount) in &days {
        lines.push(format!("{}  {:>10.2}{}", day, amount, if *day == last { "  (неповна)" } else { "" }));
    }
    lines.push(format!("Дані до {}: остання доба неповна (Cost Management відстає приблизно на півдоби).", last));
    lines.push(String::new());
    lines.push(format!("За період і в середньому за повну добу (повних діб: {}), за {}:", days.len() - 1, split));

    for (name, sum, part) in &groups {
        lines.push(format!("{:>10.2} {}  {}", sum, average(*part, full), name));
    }

    let total: f64 = days.values().sum();
    let part: f64 = days.iter().filter(|(day, _)| **day != last).map(|(_, amount)| amount).sum();
    lines.push(format!("{:>10.2} {}  РАЗОМ", total, average(part, full)));

    if !body["properties"]["nextLink"].is_null() {
        lines.push("Є ще сторінки відповіді (nextLink), вони не показані.".to_string());
    }

    lines.join("\n")
}
const NOTE: &str = "Cost Management відстає приблизно на півдоби: витрати за поточну добу (UTC) неповні.";

fn cached(age: Option<u64>) -> String {
    match age {
        Some(minutes) => format!("\nЗ кешу, {} хв тому (refresh=true — запитати наново).", minutes),
        None => String::new(),
    }
}

pub async fn run(http: &reqwest::Client, arguments: &serde_json::Value) -> serde_json::Value {
    let period = arguments["period"].as_str().unwrap_or("MonthToDate");
    let split = arguments["by"].as_str().unwrap_or("ResourceGroupName");
    let daily = arguments["daily"].as_bool().unwrap_or(false);
    let span = match (arguments["from"].as_str(), arguments["to"].as_str()) {
        (Some(from), Some(to)) if date_ok(from) && date_ok(to) => Some((from, to)),
        (None, None) => None,
        _ => return tools::reply(Err("Вкажіть from і to разом, у форматі РРРР-ММ-ДД (напр. 2026-09-01).".to_string())),
    };
    
    if !PERIODS.contains(&period) {
        return tools::reply(Err(format!("Невідомий period: {}. Допустимо: {}.", period, PERIODS.join(", "))));
    }
    if !SPLITS.contains(&split) {
        return tools::reply(Err(format!("Невідоме by: {}. Допустимо: {}.", split, SPLITS.join(", "))));
    }
    
    let scope = match tools::scope(arguments).await {
        Ok(scope) => scope,
        Err(message) => return tools::reply(Err(message)),
    };
    let path = format!("{}/providers/Microsoft.CostManagement/query?api-version=2023-11-01", scope);
    
    tools::reply(
        client::query(http, &path, &request(period, span, split, daily), tools::refresh(arguments))
            .await
            .map(|(body, age)| {
                let text = if daily { series(&body, split) } else { format!("{}\n{}", table(&body, split), NOTE) };
    
                format!("{}{}", text, cached(age))
            }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(rows: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "properties": {
                "columns": [
                    { "name": "ResourceGroupName", "type": "String" },
                    { "name": "Cost", "type": "Number" },
                    { "name": "Currency", "type": "String" },
                ],
                "rows": rows,
                "nextLink": null,
            },
        })
    }
    
    #[test]
    fn table_sorted_with_total() {
        let body = sample(serde_json::json!([["trial", 117.0, "USD"], ["demo-rg", 226.6, "USD"]]));
    
        assert_eq!(table(&body, "ResourceGroupName"), "    226.60  demo-rg\n    117.00  trial\n    343.60  РАЗОМ, USD");
    }
    
    #[test]
    fn table_empty() {
        assert_eq!(table(&sample(serde_json::json!([])), "ResourceGroupName"), "Витрат за цей період немає.");
    }
    
    #[test]
    fn table_warns_about_next_page() {
        let mut body = sample(serde_json::json!([["trial", 1.0, "USD"]]));
        body["properties"]["nextLink"] = serde_json::json!("https://next");
    
        assert!(table(&body, "ResourceGroupName").ends_with("вони не показані."));
    }
    #[test]
    fn request_shape() {
        let body = request("TheLastMonth", None, "ServiceName", false);
    
        assert_eq!(body["timeframe"], "TheLastMonth");
        assert_eq!(body["dataset"]["granularity"], "None");
        assert_eq!(body["dataset"]["grouping"][0]["name"], "ServiceName");
        assert!(body.get("timePeriod").is_none());
    }
    
    #[test]
    fn request_custom_daily() {
        let body = request("MonthToDate", Some(("2026-09-01", "2026-09-20")), "MeterCategory", true);
    
        assert_eq!(body["timeframe"], "Custom");
        assert_eq!(body["dataset"]["granularity"], "Daily");
        assert_eq!(body["timePeriod"]["from"], "2026-09-01T00:00:00+00:00");
        assert_eq!(body["timePeriod"]["to"], "2026-09-20T23:59:59+00:00");
    }
    
    #[test]
    fn dates_checked() {
        assert!(date_ok("2026-09-01"));
        assert!(!date_ok("2026-9-1"));
        assert!(!date_ok("01.09.2026"));
        assert!(!date_ok("2026-09-0a"));
    }
    #[test]
    fn short_resource() {
        assert_eq!(
            short("/subscriptions/s/resourcegroups/trial/providers/microsoft.compute/disks/win-server_osdisk_1_abc"),
            "trial  disks  win-server_osdisk_1_abc"
        );
        assert_eq!(
            short("/subscriptions/s/resourcegroups/dev/providers/microsoft.compute/virtualmachines/vm1/extensions/ext"),
            "dev  virtualmachines/extensions  ext"
        );
        assert_eq!(short("/subscriptions/s/resourcegroups/dev"), "/subscriptions/s/resourcegroups/dev");
    }
    
    #[test]
    fn table_shortens_resource_ids() {
        let body = serde_json::json!({
            "properties": {
                "columns": [{ "name": "Cost" }, { "name": "ResourceId" }, { "name": "Currency" }],
                "rows": [[1.5, "/subscriptions/s/resourcegroups/trial/providers/microsoft.compute/disks/d1", "USD"]],
                "nextLink": null,
            },
        });
    
        assert!(table(&body, "ResourceId").starts_with("      1.50  trial  disks  d1"));
    }
    fn daily_body(rows: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "properties": {
                "columns": [
                    { "name": "Cost" }, { "name": "UsageDate" }, { "name": "ResourceGroupName" }, { "name": "Currency" },
                ],
                "rows": rows,
                "nextLink": null,
            },
        })
    }
    
    #[test]
    fn series_averages_complete_days() {
        let body = daily_body(serde_json::json!([
            [10.0, 20260901, "a", "USD"], [5.0, 20260901, "b", "USD"],
            [12.0, 20260902, "a", "USD"], [6.0, 20260902, "b", "USD"],
            [3.0, 20260903, "a", "USD"],
        ]));
        let text = series(&body, "ResourceGroupName");
    
        assert!(text.starts_with("Щодоби, USD:\n2026-09-01  "));
        assert!(text.contains(&format!("2026-09-03  {:>10.2}  (неповна)", 3.0)));
        assert!(text.contains("Дані до 2026-09-03: остання доба неповна"));
        assert!(text.contains("повних діб: 2"));
        assert!(text.contains(&format!("{:>10.2} {:>10.2}  a", 25.0, 11.0)));
        assert!(text.contains(&format!("{:>10.2} {:>10.2}  b", 11.0, 5.5)));
        assert!(text.contains(&format!("{:>10.2} {:>10.2}  РАЗОМ", 36.0, 16.5)));
    }
    
    #[test]
    fn series_single_day_has_no_average() {
        let text = series(&daily_body(serde_json::json!([[4.0, 20260920, "a", "USD"]])), "ResourceGroupName");
    
        assert!(text.contains(&format!("{:>10.2} {:>10}  a", 4.0, "-")));
    }
    
    #[test]
    fn stamp_forms() {
        assert_eq!(stamp(&serde_json::json!(20260901)), "2026-09-01");
        assert_eq!(stamp(&serde_json::json!("2026-09-01T00:00:00")), "2026-09-01");
    }
    
    #[test]
    fn cached_note() {
        assert_eq!(cached(None), "");
        assert!(cached(Some(12)).contains("12 хв тому"));
    }
}
