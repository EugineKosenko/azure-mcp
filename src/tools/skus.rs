use crate::client;
use crate::tools;

fn capability<'a>(sku: &'a serde_json::Value, name: &str) -> &'a str {
    sku["capabilities"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|capability| tools::text(capability, "/name") == name)
        .map(|capability| tools::text(capability, "/value"))
        .unwrap_or("-")
}

fn line(sku: &serde_json::Value) -> String {
    format!(
        "{}  family={}  vcpu={}  ram={} ГБ",
        tools::text(sku, "/name"),
        tools::text(sku, "/family"),
        capability(sku, "vCPUs"),
        capability(sku, "MemoryGB"),
    )
}

fn table(skus: &[serde_json::Value], size: Option<&str>) -> String {
    let mut items: Vec<&serde_json::Value> = skus
        .iter()
        .filter(|sku| tools::text(sku, "/resourceType") == "virtualMachines")
        .filter(|sku| size.is_none_or(|size| tools::same(tools::text(sku, "/name"), size)))
        .collect();
    items.sort_by_key(|sku| tools::text(sku, "/name").to_lowercase());

    let lines: Vec<String> = items.into_iter().map(line).collect();

    if lines.is_empty() { "Типів VM не знайдено.".to_string() } else { lines.join("\n") }
}

pub async fn run(http: &reqwest::Client, arguments: &serde_json::Value) -> serde_json::Value {
    let subscription = match tools::subscription(arguments).await {
        Ok(subscription) => subscription,
        Err(message) => return tools::reply(Err(message)),
    };
    let Some(region) = arguments["region"].as_str() else {
        return tools::reply(Err("Аргумент region обов'язковий, напр. eastus.".to_string()));
    };
    let filter = format!("location eq '{}'", region).replace(' ', "%20");
    let path = format!("{}/providers/Microsoft.Compute/skus?api-version=2021-07-01&$filter={}", subscription, filter);
    
    tools::reply(client::list(http, &path).await.map(|skus| table(&skus, arguments["size"].as_str())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skus() -> Vec<serde_json::Value> {
        vec![
            serde_json::json!({
                "name": "Standard_D4als_v6",
                "family": "standardDalv6Family",
                "resourceType": "virtualMachines",
                "capabilities": [{ "name": "vCPUs", "value": "4" }, { "name": "MemoryGB", "value": "8" }],
            }),
            serde_json::json!({
                "name": "Standard_LRS",
                "resourceType": "disks",
                "capabilities": [],
            }),
        ]
    }
    
    #[test]
    fn table_keeps_only_virtual_machines() {
        assert_eq!(table(&skus(), None), "Standard_D4als_v6  family=standardDalv6Family  vcpu=4  ram=8 ГБ");
    }
    
    #[test]
    fn table_filters_by_size() {
        assert!(table(&skus(), Some("standard_d4als_v6")).starts_with("Standard_D4als_v6"));
        assert_eq!(table(&skus(), Some("other")), "Типів VM не знайдено.".to_string());
    }
}
