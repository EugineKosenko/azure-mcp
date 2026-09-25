use crate::client;
use crate::tools;

fn joined(value: &serde_json::Value, pointer: &str) -> String {
    let items: Vec<&str> = value.pointer(pointer).and_then(|item| item.as_array()).into_iter().flatten().filter_map(|item| item.as_str()).collect();

    if items.is_empty() { "-".to_string() } else { items.join(",") }
}

fn prefix(subnet: &serde_json::Value) -> String {
    match tools::text(subnet, "/properties/addressPrefix") {
        "-" => joined(subnet, "/properties/addressPrefixes"),
        text => text.to_string(),
    }
}

fn line(vnet: &serde_json::Value, subnet: Option<&serde_json::Value>) -> String {
    format!(
        "{}  {}  {}  subnet={}  prefix={}  nsg={}",
        tools::text(vnet, "/name"),
        tools::part(tools::text(vnet, "/id"), "resourceGroups").to_lowercase(),
        joined(vnet, "/properties/addressSpace/addressPrefixes"),
        subnet.map_or("-".to_string(), |subnet| tools::text(subnet, "/name").to_string()),
        subnet.map_or("-".to_string(), prefix),
        subnet.map_or("-".to_string(), |subnet| tools::last(tools::text(subnet, "/properties/networkSecurityGroup/id")).to_string()),
    )
}

fn table(vnets: &[serde_json::Value], name: Option<&str>) -> String {
    let lines: Vec<String> = tools::sorted(vnets)
        .into_iter()
        .filter(|vnet| name.is_none_or(|name| tools::same(tools::text(vnet, "/name"), name)))
        .flat_map(|vnet| {
            let mut subnets: Vec<&serde_json::Value> = vnet["properties"]["subnets"].as_array().into_iter().flatten().collect();
            subnets.sort_by_key(|subnet| tools::text(subnet, "/name").to_lowercase());

            if subnets.is_empty() {
                vec![line(vnet, None)]
            } else {
                subnets.into_iter().map(|subnet| line(vnet, Some(subnet))).collect()
            }
        })
        .collect();

    if lines.is_empty() { "Мереж не знайдено.".to_string() } else { lines.join("\n") }
}

pub async fn run(http: &reqwest::Client, arguments: &serde_json::Value) -> serde_json::Value {
    let scope = match tools::scope(arguments).await {
        Ok(scope) => scope,
        Err(message) => return tools::reply(Err(message)),
    };
    let path = format!("{}/providers/Microsoft.Network/virtualNetworks?api-version=2024-05-01", scope);
    
    tools::reply(client::list(http, &path).await.map(|vnets| table(&vnets, arguments["name"].as_str())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vnets() -> Vec<serde_json::Value> {
        vec![serde_json::json!({
            "name": "demo-vnet",
            "id": "/subscriptions/s/resourceGroups/DEMO-RG/providers/Microsoft.Network/virtualNetworks/demo-vnet",
            "properties": {
                "addressSpace": { "addressPrefixes": ["10.0.0.0/16"] },
                "subnets": [
                    {
                        "name": "default",
                        "properties": { "addressPrefix": "10.0.0.0/24" },
                    },
                    {
                        "name": "snet-1",
                        "properties": {
                            "addressPrefixes": ["10.0.1.0/24"],
                            "networkSecurityGroup": { "id": "/subscriptions/s/resourceGroups/demo-rg/providers/Microsoft.Network/networkSecurityGroups/demo-nsg" },
                        },
                    },
                ],
            },
        })]
    }
    
    #[test]
    fn table_shows_subnets_and_nsg() {
        assert_eq!(
            table(&vnets(), None),
            "demo-vnet  demo-rg  10.0.0.0/16  subnet=default  prefix=10.0.0.0/24  nsg=-\n\
             demo-vnet  demo-rg  10.0.0.0/16  subnet=snet-1  prefix=10.0.1.0/24  nsg=demo-nsg"
        );
    }
    
    #[test]
    fn table_filters_by_name() {
        assert!(table(&vnets(), Some("DEMO-VNET")).starts_with("demo-vnet"));
        assert_eq!(table(&vnets(), Some("other")), "Мереж не знайдено.".to_string());
    }
}
