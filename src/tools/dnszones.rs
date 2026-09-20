use crate::client;
use crate::tools;

fn block(zone: &serde_json::Value, links: &[serde_json::Value], records: &[serde_json::Value]) -> String {
    let mut lines = vec![format!(
        "{}  {}",
        tools::text(zone, "/name"),
        tools::part(tools::text(zone, "/id"), "resourceGroups"),
    )];

    for link in links {
        lines.push(format!(
            "  зв'язок {} -> {}  registration={}",
            tools::text(link, "/name"),
            tools::last(tools::text(link, "/properties/virtualNetwork/id")),
            tools::shown(link, "/properties/registrationEnabled"),
        ));
    }

    for record in records {
        let addrs: Vec<&str> = record["properties"]["aRecords"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|item| item["ipv4Address"].as_str().unwrap())
            .collect();

        lines.push(format!(
            "  A {}  {}  ttl={}",
            tools::text(record, "/name"),
            addrs.join(","),
            tools::shown(record, "/properties/ttl"),
        ));
    }

    lines.join("\n")
}

pub async fn run(http: &reqwest::Client, arguments: &serde_json::Value) -> serde_json::Value {
    let scope = match tools::scope(arguments).await {
        Ok(scope) => scope,
        Err(message) => return tools::reply(Err(message)),
    };
    let path = format!("{}/providers/Microsoft.Network/privateDnsZones?api-version=2018-09-01", scope);
    let zones = match client::list(http, &path).await {
        Ok(zones) => zones,
        Err(message) => return tools::reply(Err(message)),
    };
    let name = arguments["zone"].as_str();
    
    let mut blocks = Vec::new();
    for zone in tools::sorted(&zones).into_iter().filter(|zone| name.is_none_or(|name| tools::same(tools::text(zone, "/name"), name))) {
        let id = tools::text(zone, "/id");
        let links = client::list(http, &format!("{}/virtualNetworkLinks?api-version=2024-06-01", id)).await;
        let records = client::list(http, &format!("{}/A?api-version=2018-09-01", id)).await;
    
        match (links, records) {
            (Ok(links), Ok(records)) => blocks.push(block(zone, &links, &records)),
            (Err(message), _) | (_, Err(message)) => return tools::reply(Err(message)),
        }
    }
    
    tools::reply(Ok(if blocks.is_empty() { "Приватних DNS-зон не знайдено.".to_string() } else { blocks.join("\n\n") }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_shows_links_and_records() {
        let zone = serde_json::json!({
            "name": "privatelink.mysql.database.azure.com",
            "id": "/subscriptions/s/resourceGroups/prod-rg/providers/Microsoft.Network/privateDnsZones/privatelink.mysql.database.azure.com",
        });
        let links = vec![serde_json::json!({
            "name": "prod-link",
            "properties": {
                "registrationEnabled": false,
                "virtualNetwork": { "id": "/subscriptions/s/resourceGroups/prod-west-rg/providers/Microsoft.Network/virtualNetworks/prod-vnet" },
            },
        })];
        let records = vec![serde_json::json!({
            "name": "prod-db",
            "properties": { "ttl": 10, "aRecords": [{ "ipv4Address": "10.1.0.6" }] },
        })];
    
        assert_eq!(
            block(&zone, &links, &records),
            "privatelink.mysql.database.azure.com  prod-rg\n  зв'язок prod-link -> prod-vnet  registration=false\n  A prod-db  10.1.0.6  ttl=10"
        );
    }
    
    #[test]
    fn block_empty_zone() {
        let zone = serde_json::json!({ "name": "z", "id": "/subscriptions/s/resourceGroups/g/providers/p/z" });
    
        assert_eq!(block(&zone, &[], &[]), "z  g");
    }
}
