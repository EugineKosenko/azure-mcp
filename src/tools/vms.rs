use std::collections::HashMap;

use crate::client;
use crate::tools;

fn index(items: &[serde_json::Value]) -> HashMap<String, &serde_json::Value> {
    items.iter().map(|item| (tools::text(item, "/id").to_lowercase(), item)).collect()
}
fn power(status: Option<&&serde_json::Value>) -> String {
    status
        .and_then(|item| item["properties"]["instanceView"]["statuses"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|item| item["code"].as_str()?.strip_prefix("PowerState/"))
        .next()
        .unwrap_or("-")
        .to_string()
}
fn joined(items: Vec<&str>) -> String {
    if items.is_empty() { "-".to_string() } else { items.join(",") }
}

fn addresses<'a>(
    vm: &serde_json::Value,
    nics: &HashMap<String, &'a serde_json::Value>,
    ips: &HashMap<String, &'a serde_json::Value>,
) -> (String, String) {
    let (mut private, mut public) = (Vec::new(), Vec::new());

    for link in vm["properties"]["networkProfile"]["networkInterfaces"].as_array().into_iter().flatten() {
        let Some(nic) = nics.get(&tools::text(link, "/id").to_lowercase()) else { continue };

        for config in nic["properties"]["ipConfigurations"].as_array().into_iter().flatten() {
            private.push(tools::text(config, "/properties/privateIPAddress"));

            if let Some(ip) = ips.get(&tools::text(config, "/properties/publicIPAddress/id").to_lowercase()) {
                public.push(tools::text(ip, "/properties/ipAddress"));
            }
        }
    }

    (joined(private), joined(public))
}
fn table(vms: &[serde_json::Value], status: &[serde_json::Value], nics: &[serde_json::Value], ips: &[serde_json::Value], name: Option<&str>) -> String {
    let (status, nics, ips) = (index(status), index(nics), index(ips));

    let lines: Vec<String> = tools::sorted(vms)
        .into_iter()
        .filter(|vm| name.is_none_or(|name| tools::same(tools::text(vm, "/name"), name)))
        .map(|vm| {
            let (private, public) = addresses(vm, &nics, &ips);

            format!(
                "{}  {}  {}  {}  {}  private={}  public={}",
                tools::text(vm, "/name"),
                tools::part(tools::text(vm, "/id"), "resourceGroups").to_lowercase(),
                tools::text(vm, "/properties/hardwareProfile/vmSize"),
                tools::text(vm, "/properties/storageProfile/osDisk/osType"),
                power(status.get(&tools::text(vm, "/id").to_lowercase())),
                private,
                public,
            )
        })
        .collect();

    if lines.is_empty() { "Віртуальних машин не знайдено.".to_string() } else { lines.join("\n") }
}
async fn gather(http: &reqwest::Client, arguments: &serde_json::Value) -> Result<String, String> {
    let scope = tools::scope(arguments).await?;
    let subscription = tools::subscription(arguments).await?;

    let vms = client::list(http, &format!("{}/providers/Microsoft.Compute/virtualMachines?api-version=2024-11-01", scope)).await?;
    let status = client::list(http, &format!("{}/providers/Microsoft.Compute/virtualMachines?statusOnly=true&api-version=2024-11-01", scope)).await?;
    let nics = client::list(http, &format!("{}/providers/Microsoft.Network/networkInterfaces?api-version=2023-11-01", subscription)).await?;
    let ips = client::list(http, &format!("{}/providers/Microsoft.Network/publicIPAddresses?api-version=2024-07-01", subscription)).await?;

    Ok(table(&vms, &status, &nics, &ips, arguments["name"].as_str()))
}

pub async fn run(http: &reqwest::Client, arguments: &serde_json::Value) -> serde_json::Value {
    tools::reply(gather(http, arguments).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vms() -> Vec<serde_json::Value> {
        vec![
            serde_json::json!({
                "name": "api-vm",
                "id": "/subscriptions/s/resourceGroups/DEMO-RG/providers/Microsoft.Compute/virtualMachines/api-vm",
                "properties": {
                    "hardwareProfile": { "vmSize": "Standard_B1ms" },
                    "storageProfile": { "osDisk": { "osType": "Linux" } },
                    "networkProfile": { "networkInterfaces": [{ "id": "/subscriptions/s/resourceGroups/demo-rg/providers/Microsoft.Network/networkInterfaces/nic1" }] },
                },
            }),
            serde_json::json!({
                "name": "win-server",
                "id": "/subscriptions/s/resourceGroups/TRIAL/providers/Microsoft.Compute/virtualMachines/win-server",
                "properties": {
                    "hardwareProfile": { "vmSize": "Standard_D2s_v3" },
                    "storageProfile": { "osDisk": { "osType": "Windows" } },
                    "networkProfile": { "networkInterfaces": [{ "id": "/subscriptions/s/resourceGroups/trial/providers/Microsoft.Network/networkInterfaces/missing" }] },
                },
            }),
        ]
    }
    
    fn status() -> Vec<serde_json::Value> {
        vec![
            serde_json::json!({
                "id": "/subscriptions/s/resourceGroups/DEMO-RG/providers/Microsoft.Compute/virtualMachines/api-vm",
                "properties": { "instanceView": { "statuses": [{ "code": "ProvisioningState/succeeded" }, { "code": "PowerState/running" }] } },
            }),
            serde_json::json!({
                "id": "/subscriptions/s/resourceGroups/TRIAL/providers/Microsoft.Compute/virtualMachines/win-server",
                "properties": { "instanceView": { "statuses": [{ "code": "PowerState/deallocated" }] } },
            }),
        ]
    }
    
    fn nics() -> Vec<serde_json::Value> {
        vec![serde_json::json!({
            "id": "/subscriptions/s/resourceGroups/demo-rg/providers/Microsoft.Network/networkInterfaces/nic1",
            "properties": { "ipConfigurations": [{
                "properties": {
                    "privateIPAddress": "10.0.0.5",
                    "publicIPAddress": { "id": "/subscriptions/s/resourcegroups/demo-rg/providers/microsoft.network/publicipaddresses/ip1" },
                },
            }] },
        })]
    }
    
    fn ips() -> Vec<serde_json::Value> {
        vec![serde_json::json!({
            "id": "/subscriptions/s/resourceGroups/demo-rg/providers/Microsoft.Network/publicIPAddresses/ip1",
            "properties": { "ipAddress": "203.0.113.40" },
        })]
    }
    
    #[test]
    fn table_joins_status_and_addresses() {
        assert_eq!(
            table(&vms(), &status(), &nics(), &ips(), None),
            "api-vm  demo-rg  Standard_B1ms  Linux  running  private=10.0.0.5  public=203.0.113.40\n\
             win-server  trial  Standard_D2s_v3  Windows  deallocated  private=-  public=-"
        );
    }
    
    #[test]
    fn table_filters_by_name() {
        assert!(table(&vms(), &status(), &nics(), &ips(), Some("API-VM")).starts_with("api-vm"));
        assert_eq!(table(&vms(), &status(), &nics(), &ips(), Some("other")), "Віртуальних машин не знайдено.");
    }
    
    #[test]
    fn power_missing_is_dash() {
        assert_eq!(power(None), "-");
        assert_eq!(table(&vms(), &[], &nics(), &ips(), Some("win-server")), "win-server  trial  Standard_D2s_v3  Windows  -  private=-  public=-");
    }
}
