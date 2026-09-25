use crate::client;
use crate::tools;

fn line(kind: &str, item: &serde_json::Value) -> String {
    let current = item["currentValue"].as_i64().unwrap();
    let limit = item["limit"].as_i64().unwrap();

    format!(
        "{}  {}  {}/{} {}{}",
        kind,
        tools::text(item, "/name/localizedValue"),
        current,
        limit,
        tools::text(item, "/unit"),
        if limit > 0 && current >= limit { "  (вичерпано)" } else { "" },
    )
}

fn table(compute: &[serde_json::Value], network: &[serde_json::Value], kind: Option<&str>) -> String {
    let mut lines: Vec<String> = Vec::new();

    if kind.is_none_or(|kind| tools::same(kind, "compute")) {
        lines.extend(compute.iter().map(|item| line("compute", item)));
    }
    if kind.is_none_or(|kind| tools::same(kind, "network")) {
        lines.extend(network.iter().map(|item| line("network", item)));
    }
    lines.sort();

    if lines.is_empty() { "Квот не знайдено.".to_string() } else { lines.join("\n") }
}
async fn fetch(http: &reqwest::Client, subscription: &str, region: &str, provider: &str, api_version: &str) -> Result<Vec<serde_json::Value>, String> {
    client::list(http, &format!("{}/providers/{}/locations/{}/usages?api-version={}", subscription, provider, region, api_version)).await
}

pub async fn run(http: &reqwest::Client, arguments: &serde_json::Value) -> serde_json::Value {
    let subscription = match tools::subscription(arguments).await {
        Ok(subscription) => subscription,
        Err(message) => return tools::reply(Err(message)),
    };
    let Some(region) = arguments["region"].as_str() else {
        return tools::reply(Err("Аргумент region обов'язковий, напр. eastus.".to_string()));
    };
    let kind = arguments["kind"].as_str();
    
    let compute = if kind.is_none_or(|kind| tools::same(kind, "compute")) {
        match fetch(http, &subscription, region, "Microsoft.Compute", "2024-11-01").await {
            Ok(items) => items,
            Err(message) => return tools::reply(Err(message)),
        }
    } else {
        Vec::new()
    };
    let network = if kind.is_none_or(|kind| tools::same(kind, "network")) {
        match fetch(http, &subscription, region, "Microsoft.Network", "2024-05-01").await {
            Ok(items) => items,
            Err(message) => return tools::reply(Err(message)),
        }
    } else {
        Vec::new()
    };
    
    tools::reply(Ok(table(&compute, &network, kind)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compute() -> Vec<serde_json::Value> {
        vec![
            serde_json::json!({ "name": { "value": "standardBSFamily", "localizedValue": "Standard BS Family vCPUs" }, "currentValue": 10, "limit": 10, "unit": "Count" }),
            serde_json::json!({ "name": { "value": "standardDLSv5Family", "localizedValue": "Standard DLSv5 Family vCPUs" }, "currentValue": 4, "limit": 20, "unit": "Count" }),
        ]
    }
    
    fn network() -> Vec<serde_json::Value> {
        vec![serde_json::json!({ "name": { "value": "PublicIPAddresses", "localizedValue": "Public IP Addresses" }, "currentValue": 11, "limit": 20, "unit": "Count" })]
    }
    
    #[test]
    fn table_marks_exhausted_and_sorts() {
        assert_eq!(
            table(&compute(), &network(), None),
            "compute  Standard BS Family vCPUs  10/10 Count  (вичерпано)\n\
             compute  Standard DLSv5 Family vCPUs  4/20 Count\n\
             network  Public IP Addresses  11/20 Count"
        );
    }
    
    #[test]
    fn table_filters_by_kind() {
        assert_eq!(table(&compute(), &network(), Some("network")), "network  Public IP Addresses  11/20 Count");
        assert_eq!(table(&[], &[], None), "Квот не знайдено.".to_string());
    }
}
