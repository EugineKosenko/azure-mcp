use crate::client;
use crate::tools;

fn line(snapshot: &serde_json::Value) -> String {
    format!(
        "{}  {}  {}  {} ГБ  {}  {}  {}  source={}  incremental={}",
        tools::text(snapshot, "/name"),
        tools::part(tools::text(snapshot, "/id"), "resourceGroups").to_lowercase(),
        tools::text(snapshot, "/sku/name"),
        tools::shown(snapshot, "/properties/diskSizeGB"),
        tools::text(snapshot, "/properties/hyperVGeneration"),
        tools::text(snapshot, "/properties/securityProfile/securityType"),
        tools::text(snapshot, "/properties/osType"),
        tools::last(tools::text(snapshot, "/properties/creationData/sourceResourceId")),
        tools::shown(snapshot, "/properties/incremental"),
    )
}

fn table(snapshots: &[serde_json::Value], name: Option<&str>) -> String {
    let lines: Vec<String> = tools::sorted(snapshots)
        .into_iter()
        .filter(|snapshot| name.is_none_or(|name| tools::same(tools::text(snapshot, "/name"), name)))
        .map(line)
        .collect();

    if lines.is_empty() { "Снепшотів не знайдено.".to_string() } else { lines.join("\n") }
}

pub async fn run(http: &reqwest::Client, arguments: &serde_json::Value) -> serde_json::Value {
    let scope = match tools::scope(arguments).await {
        Ok(scope) => scope,
        Err(message) => return tools::reply(Err(message)),
    };
    let path = format!("{}/providers/Microsoft.Compute/snapshots?api-version=2025-01-02", scope);
    
    tools::reply(client::list(http, &path).await.map(|snapshots| table(&snapshots, arguments["name"].as_str())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshots() -> Vec<serde_json::Value> {
        vec![serde_json::json!({
            "name": "web-vm-snapshot",
            "id": "/subscriptions/s/resourceGroups/TRIAL/providers/Microsoft.Compute/snapshots/web-vm-snapshot",
            "sku": { "name": "Standard_LRS" },
            "properties": {
                "diskSizeGB": 128,
                "hyperVGeneration": "V2",
                "securityProfile": { "securityType": "TrustedLaunch" },
                "osType": "Windows",
                "incremental": false,
                "creationData": { "sourceResourceId": "/subscriptions/s/resourceGroups/trial/providers/Microsoft.Compute/disks/web-vm_OsDisk_1_abc" },
            },
        })]
    }
    
    #[test]
    fn table_shows_source_and_security() {
        assert_eq!(
            table(&snapshots(), None),
            "web-vm-snapshot  trial  Standard_LRS  128 ГБ  V2  TrustedLaunch  Windows  source=web-vm_OsDisk_1_abc  incremental=false"
        );
    }
    
    #[test]
    fn table_filters_by_name() {
        assert!(table(&snapshots(), Some("WEB-VM-SNAPSHOT")).starts_with("web-vm-snapshot"));
        assert_eq!(table(&snapshots(), Some("other")), "Снепшотів не знайдено.".to_string());
    }
}
