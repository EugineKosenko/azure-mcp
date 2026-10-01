use crate::client;
use crate::tools;

fn zones(disk: &serde_json::Value) -> String {
    let items: Vec<&str> = disk["zones"].as_array().into_iter().flatten().filter_map(|zone| zone.as_str()).collect();

    if items.is_empty() { "-".to_string() } else { items.join(",") }
}

fn origin(disk: &serde_json::Value) -> String {
    let base = "/properties/creationData";
    let marketplace = tools::text(disk, &format!("{}/imageReference/id", base));

    if marketplace.to_lowercase().contains("/publishers/") {
        return format!(
            "{}/{}/{}/{}",
            tools::part(marketplace, "Publishers"),
            tools::part(marketplace, "Offers"),
            tools::part(marketplace, "Skus"),
            tools::part(marketplace, "Versions"),
        );
    }

    ["imageReference/id", "galleryImageReference/id", "imageReference/sharedGalleryImageId", "imageReference/communityGalleryImageId"]
        .iter()
        .map(|key| tools::text(disk, &format!("{}/{}", base, key)))
        .find(|id| *id != "-")
        .map_or("-".to_string(), |id| tools::last(id).to_string())
}

fn line(disk: &serde_json::Value) -> String {
    let state = tools::text(disk, "/properties/diskState");

    format!(
        "{}  {}  {}  {} ГБ  {}  {}  {}  vm={}  zones={}  tier={}  iops={}  mbps={}  bursting={}  os={}  create={}  image={}  hibernation={}{}",
        tools::text(disk, "/name"),
        tools::part(tools::text(disk, "/id"), "resourceGroups").to_lowercase(),
        tools::text(disk, "/sku/name"),
        tools::shown(disk, "/properties/diskSizeGB"),
        tools::text(disk, "/properties/hyperVGeneration"),
        tools::text(disk, "/properties/securityProfile/securityType"),
        state,
        tools::last(tools::text(disk, "/managedBy")),
        zones(disk),
        tools::text(disk, "/properties/tier"),
        tools::shown(disk, "/properties/diskIOPSReadWrite"),
        tools::shown(disk, "/properties/diskMBpsReadWrite"),
        tools::shown(disk, "/properties/burstingEnabled"),
        tools::text(disk, "/properties/osType"),
        tools::text(disk, "/properties/creationData/createOption"),
        origin(disk),
        tools::shown(disk, "/properties/supportsHibernation"),
        if state == "Unattached" { "  (не прив'язаний)" } else { "" },
    )
}

fn table(disks: &[serde_json::Value], name: Option<&str>) -> String {
    let lines: Vec<String> = tools::sorted(disks)
        .into_iter()
        .filter(|disk| name.is_none_or(|name| tools::same(tools::text(disk, "/name"), name)))
        .map(line)
        .collect();

    if lines.is_empty() { "Дисків не знайдено.".to_string() } else { lines.join("\n") }
}

pub async fn run(http: &reqwest::Client, arguments: &serde_json::Value) -> serde_json::Value {
    let scope = match tools::scope(arguments).await {
        Ok(scope) => scope,
        Err(message) => return tools::reply(Err(message)),
    };
    let path = format!("{}/providers/Microsoft.Compute/disks?api-version=2023-04-02", scope);
    
    tools::reply(client::list(http, &path).await.map(|disks| table(&disks, arguments["name"].as_str())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disks() -> Vec<serde_json::Value> {
        vec![
            serde_json::json!({
                "name": "win-server_osdisk_1",
                "id": "/subscriptions/s/resourceGroups/TRIAL/providers/Microsoft.Compute/disks/win-server_osdisk_1",
                "sku": { "name": "Premium_LRS" },
                "managedBy": "/subscriptions/s/resourceGroups/TRIAL/providers/Microsoft.Compute/virtualMachines/win-server",
                "zones": ["1"],
                "properties": { "diskSizeGB": 127, "diskState": "Attached", "hyperVGeneration": "V2", "securityProfile": { "securityType": "TrustedLaunch" }, "tier": "P10", "diskIOPSReadWrite": 500, "diskMBpsReadWrite": 100, "burstingEnabled": true, "osType": "Windows", "supportsHibernation": true, "creationData": { "createOption": "FromImage", "imageReference": { "id": "/Subscriptions/s/Providers/Microsoft.Compute/Locations/eastus/Publishers/MicrosoftWindowsServer/ArtifactTypes/VMImage/Offers/WindowsServer/Skus/2022-datacenter/Versions/20348.1.0" } } },
            }),
            serde_json::json!({
                "name": "old-data",
                "id": "/subscriptions/s/resourceGroups/demo-rg/providers/Microsoft.Compute/disks/old-data",
                "sku": { "name": "Standard_LRS" },
                "properties": { "diskSizeGB": 32, "diskState": "Unattached" },
            }),
        ]
    }
    
    #[test]
    fn table_marks_unattached() {
        assert_eq!(
            table(&disks(), None),
            "old-data  demo-rg  Standard_LRS  32 ГБ  -  -  Unattached  vm=-  zones=-  tier=-  iops=-  mbps=-  bursting=-  os=-  create=-  image=-  hibernation=-  (не прив'язаний)\n\
             win-server_osdisk_1  trial  Premium_LRS  127 ГБ  V2  TrustedLaunch  Attached  vm=win-server  zones=1  tier=P10  iops=500  mbps=100  bursting=true  os=Windows  create=FromImage  image=MicrosoftWindowsServer/WindowsServer/2022-datacenter/20348.1.0  hibernation=true"
        );
    }
    
    #[test]
    fn origin_forms() {
        let custom = serde_json::json!({ "properties": { "creationData": { "imageReference": { "id": "/subscriptions/s/resourceGroups/demo-rg/providers/Microsoft.Compute/images/web-image" } } } });
        let shared = serde_json::json!({ "properties": { "creationData": { "imageReference": { "sharedGalleryImageId": "/SharedGalleries/g/Images/web/Versions/1.0.0" } } } });
    
        assert_eq!(origin(&custom), "web-image");
        assert_eq!(origin(&shared), "1.0.0");
        assert_eq!(origin(&serde_json::json!({})), "-");
    }
    
    #[test]
    fn table_filters_by_name() {
        assert!(table(&disks(), Some("OLD-DATA")).starts_with("old-data"));
        assert_eq!(table(&disks(), Some("other")), "Дисків не знайдено.");
    }
}
