use std::net::Ipv4Addr;

use crate::client;
use crate::tools;

fn mark(rule: &serde_json::Value, ips: &[serde_json::Value]) -> String {
    let from = tools::text(rule, "/properties/startIpAddress").parse::<Ipv4Addr>();
    let to = tools::text(rule, "/properties/endIpAddress").parse::<Ipv4Addr>();
    let (Ok(from), Ok(to)) = (from, to) else { return "-".to_string() };

    if from.is_unspecified() && to.is_unspecified() {
        return "усі служби Azure".to_string();
    }

    if from.is_private() && to.is_private() {
        return "приватна мережа".to_string();
    }

    let names: Vec<&str> = ips
        .iter()
        .filter(|ip| tools::text(ip, "/properties/ipAddress").parse::<Ipv4Addr>().is_ok_and(|addr| from <= addr && addr <= to))
        .map(|ip| tools::text(ip, "/name"))
        .collect();

    if names.is_empty() {
        "не збігається з жодною публічною IP підписки".to_string()
    } else {
        format!("= {}", names.join(", "))
    }
}
fn silence(access: &str) -> &'static str {
    match access {
        "Enabled" => "публічні підключення заборонені, жодне правило їх не дозволяє",
        "Disabled" => "публічний доступ вимкнено",
        _ => "стан публічного доступу невідомий",
    }
}
fn block(server: &serde_json::Value, rules: &[serde_json::Value], ips: &[serde_json::Value]) -> String {
    let endpoints: Vec<&str> = server["properties"]["privateEndpointConnections"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| tools::last(tools::text(item, "/properties/privateEndpoint/id")))
        .collect();

    let mut lines = vec![
        format!(
            "{}  {}  publicNetworkAccess={}",
            tools::text(server, "/name"),
            tools::part(tools::text(server, "/id"), "resourceGroups"),
            tools::text(server, "/properties/network/publicNetworkAccess"),
        ),
        format!("  приватні ендпоінти: {}", if endpoints.is_empty() { "-".to_string() } else { endpoints.join(", ") }),
    ];

    if rules.is_empty() {
        lines.push(format!("  правил немає: {}", silence(tools::text(server, "/properties/network/publicNetworkAccess"))));
    }

    for rule in rules {
        lines.push(format!(
            "  {}  {}-{}  {}",
            tools::text(rule, "/name"),
            tools::text(rule, "/properties/startIpAddress"),
            tools::text(rule, "/properties/endIpAddress"),
            mark(rule, ips),
        ));
    }

    lines.join("\n")
}

pub async fn run(http: &reqwest::Client, arguments: &serde_json::Value) -> serde_json::Value {
    let scope = match tools::scope(arguments).await {
        Ok(scope) => scope,
        Err(message) => return tools::reply(Err(message)),
    };
    let path = format!("{}/providers/Microsoft.DBforMySQL/flexibleServers?api-version=2024-10-01-preview", scope);
    let servers = match client::list(http, &path).await {
        Ok(servers) => servers,
        Err(message) => return tools::reply(Err(message)),
    };
    let name = arguments["server"].as_str();
    
    let subscription = match tools::subscription(arguments).await {
        Ok(subscription) => subscription,
        Err(message) => return tools::reply(Err(message)),
    };
    let ips = match client::list(http, &format!("{}/providers/Microsoft.Network/publicIPAddresses?api-version=2024-07-01", subscription)).await {
        Ok(ips) => ips,
        Err(message) => return tools::reply(Err(message)),
    };
    
    let mut blocks = Vec::new();
    for server in tools::sorted(&servers).into_iter().filter(|server| name.is_none_or(|name| tools::same(tools::text(server, "/name"), name))) {
        let path = format!("{}/firewallRules?api-version=2023-12-30", tools::text(server, "/id"));
    
        match client::list(http, &path).await {
            Ok(rules) => blocks.push(block(server, &rules, &ips)),
            Err(message) => return tools::reply(Err(message)),
        }
    }
    
    tools::reply(Ok(if blocks.is_empty() { "Серверів MySQL не знайдено.".to_string() } else { blocks.join("\n\n") }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_shows_network_and_rules() {
        let server = serde_json::json!({
            "name": "demo-db",
            "id": "/subscriptions/s/resourceGroups/demo-rg/providers/Microsoft.DBforMySQL/flexibleServers/demo-db",
            "properties": {
                "network": { "publicNetworkAccess": "Enabled" },
                "privateEndpointConnections": [{
                    "properties": { "privateEndpoint": { "id": "/subscriptions/s/resourceGroups/demo-rg/providers/Microsoft.Network/privateEndpoints/demo-dbep" } },
                }],
            },
        });
        let rules = vec![serde_json::json!({ "name": "local", "properties": { "startIpAddress": "10.0.0.1", "endIpAddress": "10.0.0.255" } })];
    
        assert_eq!(
            block(&server, &rules, &[]),
            "demo-db  demo-rg  publicNetworkAccess=Enabled\n  приватні ендпоінти: demo-dbep\n  local  10.0.0.1-10.0.0.255  приватна мережа"
        );
    }
    
    #[test]
    fn block_without_endpoints() {
        let server = serde_json::json!({ "name": "s", "id": "/subscriptions/x/resourceGroups/g/providers/p/s", "properties": {} });
    
        assert_eq!(
            block(&server, &[], &[]),
            "s  g  publicNetworkAccess=-\n  приватні ендпоінти: -\n  правил немає: стан публічного доступу невідомий"
        );
    }
    
    #[test]
    fn block_without_rules_says_what_it_means() {
        let server = |access: &str| serde_json::json!({ "name": "s", "id": "/subscriptions/x/resourceGroups/g/providers/p/s", "properties": { "network": { "publicNetworkAccess": access } } });
    
        assert!(block(&server("Enabled"), &[], &[]).ends_with("правил немає: публічні підключення заборонені, жодне правило їх не дозволяє"));
        assert!(block(&server("Disabled"), &[], &[]).ends_with("правил немає: публічний доступ вимкнено"));
    }
    fn rule(start: &str, end: &str) -> serde_json::Value {
        serde_json::json!({ "name": "r", "properties": { "startIpAddress": start, "endIpAddress": end } })
    }
    
    fn ips() -> Vec<serde_json::Value> {
        vec![
            serde_json::json!({ "name": "web-vm-ip", "properties": { "ipAddress": "203.0.113.20" } }),
            serde_json::json!({ "name": "api-ip", "properties": { "ipAddress": "203.0.113.29" } }),
            serde_json::json!({ "name": "no-address-ip", "properties": {} }),
        ]
    }
    
    #[test]
    fn mark_azure_services() {
        assert_eq!(mark(&rule("0.0.0.0", "0.0.0.0"), &ips()), "усі служби Azure");
    }
    
    #[test]
    fn mark_private_network() {
        assert_eq!(mark(&rule("10.0.0.1", "10.0.0.255"), &ips()), "приватна мережа");
        assert_eq!(mark(&rule("192.168.1.1", "192.168.1.1"), &ips()), "приватна мережа");
    }
    
    #[test]
    fn mark_matches_public_addresses() {
        assert_eq!(mark(&rule("203.0.113.20", "203.0.113.20"), &ips()), "= web-vm-ip");
        assert_eq!(mark(&rule("203.0.113.19", "203.0.113.30"), &ips()), "= web-vm-ip, api-ip");
    }
    
    #[test]
    fn mark_unknown_address() {
        assert_eq!(mark(&rule("198.51.100.7", "198.51.100.7"), &ips()), "не збігається з жодною публічною IP підписки");
        assert_eq!(mark(&rule("bad", "198.51.100.7"), &ips()), "-");
    }
}
