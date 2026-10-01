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

fn converted(sku: &serde_json::Value, name: &str, divisor: u64) -> String {
    let value = capability(sku, name);

    value.parse::<u64>().map_or(value.to_string(), |bytes| (bytes / divisor).to_string())
}

fn line(sku: &serde_json::Value) -> String {
    format!(
        "{}  family={}  vcpu={}  ram={} ГБ  uncachedIOPS={}  uncachedMiBs={}  cachedGiB={}  maxDataDisks={}  hyperV={}  premiumIO={}  nvmeMiB={}  diskCtl={}",
        tools::text(sku, "/name"),
        tools::text(sku, "/family"),
        capability(sku, "vCPUs"),
        capability(sku, "MemoryGB"),
        capability(sku, "UncachedDiskIOPS"),
        converted(sku, "UncachedDiskBytesPerSecond", 1_048_576),
        converted(sku, "CachedDiskBytes", 1_073_741_824),
        capability(sku, "MaxDataDiskCount"),
        capability(sku, "HyperVGenerations"),
        capability(sku, "PremiumIO"),
        capability(sku, "NvmeDiskSizeInMiB"),
        capability(sku, "DiskControllerTypes"),
    )
}

fn table(skus: &[serde_json::Value], size: Option<&str>, family: Option<&str>) -> String {
    let mut items: Vec<&serde_json::Value> = skus
        .iter()
        .filter(|sku| tools::text(sku, "/resourceType") == "virtualMachines")
        .filter(|sku| size.is_none_or(|size| tools::same(tools::text(sku, "/name"), size)))
        .filter(|sku| family.is_none_or(|family| tools::text(sku, "/family").to_lowercase().contains(&family.to_lowercase())))
        .collect();
    items.sort_by_key(|sku| tools::text(sku, "/name").to_lowercase());

    let lines: Vec<String> = items.into_iter().map(line).collect();

    if lines.is_empty() { "Типів VM не знайдено.".to_string() } else { lines.join("\n") }
}

pub async fn run(http: &reqwest::Client, arguments: &serde_json::Value) -> serde_json::Value {
    if let Err(message) = tools::unknown(arguments, &["region", "size", "family", "sbscrptn"]) {
        return tools::reply(Err(message));
    }
    
    let subscription = match tools::subscription(arguments).await {
        Ok(subscription) => subscription,
        Err(message) => return tools::reply(Err(message)),
    };
    let Some(region) = arguments["region"].as_str() else {
        return tools::reply(Err("Аргумент region обов'язковий, напр. eastus.".to_string()));
    };
    let filter = format!("location eq '{}'", region).replace(' ', "%20");
    let path = format!("{}/providers/Microsoft.Compute/skus?api-version=2021-07-01&$filter={}", subscription, filter);
    
    tools::reply(client::list(http, &path).await.map(|skus| table(&skus, arguments["size"].as_str(), arguments["family"].as_str())))
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
                "capabilities": [
                    { "name": "vCPUs", "value": "4" },
                    { "name": "MemoryGB", "value": "8" },
                    { "name": "UncachedDiskIOPS", "value": "6400" },
                    { "name": "UncachedDiskBytesPerSecond", "value": "104857600" },
                    { "name": "CachedDiskBytes", "value": "107374182400" },
                    { "name": "MaxDataDiskCount", "value": "8" },
                    { "name": "HyperVGenerations", "value": "V1,V2" },
                    { "name": "PremiumIO", "value": "True" },
                ],
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
        assert_eq!(
            table(&skus(), None, None),
            "Standard_D4als_v6  family=standardDalv6Family  vcpu=4  ram=8 ГБ  uncachedIOPS=6400  uncachedMiBs=100  cachedGiB=100  maxDataDisks=8  hyperV=V1,V2  premiumIO=True  nvmeMiB=-  diskCtl=-"
        );
    }
    
    #[test]
    fn table_filters_by_size() {
        assert!(table(&skus(), Some("standard_d4als_v6"), None).starts_with("Standard_D4als_v6"));
        assert_eq!(table(&skus(), Some("other"), None), "Типів VM не знайдено.".to_string());
    }
    
    #[test]
    fn table_filters_by_family() {
        assert!(table(&skus(), None, Some("DALV6")).starts_with("Standard_D4als_v6"));
        assert_eq!(table(&skus(), None, Some("basv2")), "Типів VM не знайдено.".to_string());
    }
}
