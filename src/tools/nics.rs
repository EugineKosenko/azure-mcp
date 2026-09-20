use crate::client;
use crate::tools;

fn config(item: &serde_json::Value) -> String {
    let subnet = tools::text(item, "/properties/subnet/id");

    format!(
        "  {}  {}/{}  public={}",
        tools::text(item, "/properties/privateIPAddress"),
        tools::part(subnet, "virtualNetworks"),
        tools::part(subnet, "subnets"),
        tools::last(tools::text(item, "/properties/publicIPAddress/id")),
    )
}
fn table(nics: &[serde_json::Value], name: Option<&str>) -> String {
    let mut lines = Vec::new();

    for nic in tools::sorted(nics).into_iter().filter(|nic| name.is_none_or(|name| tools::same(tools::text(nic, "/name"), name))) {
        lines.push(format!(
            "{}  {}  vm={}  nsg={}  forwarding={}",
            tools::text(nic, "/name"),
            tools::part(tools::text(nic, "/id"), "resourceGroups"),
            tools::last(tools::text(nic, "/properties/virtualMachine/id")),
            tools::last(tools::text(nic, "/properties/networkSecurityGroup/id")),
            tools::shown(nic, "/properties/enableIPForwarding"),
        ));

        for item in nic["properties"]["ipConfigurations"].as_array().unwrap() {
            lines.push(config(item));
        }
    }

    if lines.is_empty() { "Інтерфейсів не знайдено.".to_string() } else { lines.join("\n") }
}

pub async fn run(http: &reqwest::Client, arguments: &serde_json::Value) -> serde_json::Value {
    let scope = match tools::scope(arguments).await {
        Ok(scope) => scope,
        Err(message) => return tools::reply(Err(message)),
    };
    let path = format!("{}/providers/Microsoft.Network/networkInterfaces?api-version=2023-11-01", scope);
    
    tools::reply(client::list(http, &path).await.map(|nics| table(&nics, arguments["name"].as_str())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<serde_json::Value> {
        vec![serde_json::json!({
            "name": "web-vm548_z1",
            "id": "/subscriptions/s/resourceGroups/demo-rg/providers/Microsoft.Network/networkInterfaces/web-vm548_z1",
            "properties": {
                "enableIPForwarding": false,
                "virtualMachine": { "id": "/subscriptions/s/resourceGroups/demo-rg/providers/Microsoft.Compute/virtualMachines/web-vm" },
                "ipConfigurations": [{
                    "properties": {
                        "privateIPAddress": "10.0.0.9",
                        "subnet": { "id": "/subscriptions/s/resourceGroups/demo-rg/providers/Microsoft.Network/virtualNetworks/demo-vnet/subnets/default" },
                        "publicIPAddress": { "id": "/subscriptions/s/resourceGroups/demo-rg/providers/Microsoft.Network/publicIPAddresses/web-vm-ip" },
                    },
                }],
            },
        })]
    }
    
    #[test]
    fn table_shows_configuration() {
        assert_eq!(
            table(&sample(), None),
            "web-vm548_z1  demo-rg  vm=web-vm  nsg=-  forwarding=false\n  10.0.0.9  demo-vnet/default  public=web-vm-ip"
        );
    }
    
    #[test]
    fn table_filters_by_name() {
        assert!(table(&sample(), Some("WEB-VM548_Z1")).starts_with("web-vm548_z1"));
        assert_eq!(table(&sample(), Some("other")), "Інтерфейсів не знайдено.");
    }
}
