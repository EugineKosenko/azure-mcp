use std::collections::HashMap;

use crate::client;
use crate::tools;

fn owner(id: &str) -> String {
    match id.split_once("/providers/Microsoft.Network/") {
        Some((_, rest)) => rest.split('/').take(2).collect::<Vec<_>>().join("/"),
        None => "-".to_string(),
    }
}
fn machines(nics: &[serde_json::Value]) -> HashMap<String, String> {
    nics.iter()
        .filter_map(|nic| {
            let vm = nic.pointer("/properties/virtualMachine/id")?.as_str()?;

            Some((tools::text(nic, "/id").to_lowercase(), tools::last(vm).to_string()))
        })
        .collect()
}

fn machine<'a>(machines: &'a HashMap<String, String>, id: &str) -> &'a str {
    id.split_once("/ipConfigurations/")
        .and_then(|(nic, _)| machines.get(&nic.to_lowercase()))
        .map(|vm| vm.as_str())
        .unwrap_or("-")
}
fn line(ip: &serde_json::Value, machines: &HashMap<String, String>) -> String {
    let config = tools::text(ip, "/properties/ipConfiguration/id");
    let gate = tools::text(ip, "/properties/natGateway/id");
    let binding = match (config, gate) {
        ("-", "-") => "не прив'язана".to_string(),
        ("-", gate) => owner(gate),
        (config, _) => format!("{}  vm={}", owner(config), machine(machines, config)),
    };

    format!(
        "{}  {}  {}  {} {}  {}",
        tools::text(ip, "/name"),
        tools::part(tools::text(ip, "/id"), "resourceGroups"),
        tools::text(ip, "/properties/ipAddress"),
        tools::text(ip, "/sku/name"),
        tools::text(ip, "/properties/publicIPAllocationMethod"),
        binding,
    )
}

fn table(ips: &[serde_json::Value], machines: &HashMap<String, String>, addr: Option<&str>) -> String {
    let lines: Vec<String> = tools::sorted(ips)
        .into_iter()
        .filter(|ip| addr.is_none_or(|addr| tools::text(ip, "/properties/ipAddress") == addr))
        .map(|ip| line(ip, machines))
        .collect();

    if lines.is_empty() { "Публічних IP не знайдено.".to_string() } else { lines.join("\n") }
}

pub async fn run(http: &reqwest::Client, arguments: &serde_json::Value) -> serde_json::Value {
    let scope = match tools::scope(arguments).await {
        Ok(scope) => scope,
        Err(message) => return tools::reply(Err(message)),
    };
    let ips = match client::list(http, &format!("{}/providers/Microsoft.Network/publicIPAddresses?api-version=2024-07-01", scope)).await {
        Ok(ips) => ips,
        Err(message) => return tools::reply(Err(message)),
    };
    let nics = match client::list(http, &format!("{}/providers/Microsoft.Network/networkInterfaces?api-version=2023-11-01", scope)).await {
        Ok(nics) => nics,
        Err(message) => return tools::reply(Err(message)),
    };
    
    tools::reply(Ok(table(&ips, &machines(&nics), arguments["addr"].as_str())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ips() -> Vec<serde_json::Value> {
        vec![
            serde_json::json!({
                "name": "web-vm-ip",
                "id": "/subscriptions/s/resourceGroups/demo-rg/providers/Microsoft.Network/publicIPAddresses/web-vm-ip",
                "sku": { "name": "Standard" },
                "properties": {
                    "ipAddress": "203.0.113.10",
                    "publicIPAllocationMethod": "Static",
                    "ipConfiguration": { "id": "/subscriptions/s/resourceGroups/demo-rg/providers/Microsoft.Network/networkInterfaces/web-vm548_z1/ipConfigurations/ipconfig1" },
                },
            }),
            serde_json::json!({
                "name": "free-ip",
                "id": "/subscriptions/s/resourceGroups/trial/providers/Microsoft.Network/publicIPAddresses/free-ip",
                "sku": { "name": "Basic" },
                "properties": { "publicIPAllocationMethod": "Dynamic" },
            }),
            serde_json::json!({
                "name": "nat-ip",
                "id": "/subscriptions/s/resourceGroups/demo-rg/providers/Microsoft.Network/publicIPAddresses/nat-ip",
                "sku": { "name": "Standard" },
                "properties": {
                    "ipAddress": "203.0.113.50",
                    "publicIPAllocationMethod": "Static",
                    "natGateway": { "id": "/subscriptions/s/resourceGroups/demo-rg/providers/Microsoft.Network/natGateways/ng1" },
                },
            }),
        ]
    }
    
    fn nics() -> Vec<serde_json::Value> {
        vec![serde_json::json!({
            "id": "/subscriptions/s/resourcegroups/demo-rg/providers/microsoft.network/networkinterfaces/web-vm548_z1",
            "properties": { "virtualMachine": { "id": "/subscriptions/s/resourceGroups/demo-rg/providers/Microsoft.Compute/virtualMachines/web-vm" } },
        })]
    }
    
    #[test]
    fn table_shows_owner_and_machine() {
        assert_eq!(
            table(&ips(), &machines(&nics()), None),
            "nat-ip  demo-rg  203.0.113.50  Standard Static  natGateways/ng1\n\
             web-vm-ip  demo-rg  203.0.113.10  Standard Static  networkInterfaces/web-vm548_z1  vm=web-vm\n\
             free-ip  trial  -  Basic Dynamic  не прив'язана"
        );
    }
    
    #[test]
    fn table_filters_by_address() {
        assert!(table(&ips(), &machines(&nics()), Some("203.0.113.10")).ends_with("vm=web-vm"));
        assert_eq!(table(&ips(), &machines(&nics()), Some("198.51.100.1")), "Публічних IP не знайдено.");
    }
}
