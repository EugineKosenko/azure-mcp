use crate::client;
use crate::tools;

fn short(id: &str) -> String {
    match tools::part(id, "subnets") {
        "-" => tools::last(id).to_string(),
        subnet => format!("{}/{}", tools::part(id, "virtualNetworks"), subnet),
    }
}

fn attached(nsg: &serde_json::Value, key: &str) -> String {
    let items: Vec<String> = nsg["properties"][key]
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| short(item["id"].as_str().unwrap()))
        .collect();

    if items.is_empty() { "-".to_string() } else { items.join(", ") }
}
fn ranges(props: &serde_json::Value, one: &str, many: &str) -> String {
    let mut items: Vec<&str> = props[many].as_array().into_iter().flatten().map(|item| item.as_str().unwrap()).collect();

    if let Some(item) = props[one].as_str() {
        items.push(item);
    }

    if items.is_empty() { "-".to_string() } else { items.join(",") }
}
fn line(rule: &serde_json::Value, default: bool) -> String {
    let props = &rule["properties"];

    format!(
        "  {:>5} {:<8} {:<5} {:<4} {}:{} -> {}:{}  {}{}",
        props["priority"].as_i64().unwrap(),
        props["direction"].as_str().unwrap(),
        props["access"].as_str().unwrap(),
        props["protocol"].as_str().unwrap().to_uppercase(),
        ranges(props, "sourceAddressPrefix", "sourceAddressPrefixes"),
        ranges(props, "sourcePortRange", "sourcePortRanges"),
        ranges(props, "destinationAddressPrefix", "destinationAddressPrefixes"),
        ranges(props, "destinationPortRange", "destinationPortRanges"),
        rule["name"].as_str().unwrap(),
        if default { " (типове)" } else { "" },
    )
}
const OPEN: [&str; 4] = ["*", "Internet", "0.0.0.0/0", "Any"];

fn open(nsg: &serde_json::Value) -> String {
    let mut items = Vec::new();

    for key in ["securityRules", "defaultSecurityRules"] {
        for rule in nsg["properties"][key].as_array().into_iter().flatten() {
            let props = &rule["properties"];
            let from = ranges(props, "sourceAddressPrefix", "sourceAddressPrefixes");

            if props["direction"] == "Inbound" && props["access"] == "Allow" && from.split(',').any(|item| OPEN.contains(&item)) {
                items.push(format!(
                    "{}/{}",
                    ranges(props, "destinationPortRange", "destinationPortRanges"),
                    props["protocol"].as_str().unwrap().to_uppercase(),
                ));
            }
        }
    }

    if items.is_empty() { "-".to_string() } else { items.join(", ") }
}
fn table(groups: &[serde_json::Value], name: Option<&str>, custom: bool) -> String {
    let mut lines = Vec::new();

    for nsg in tools::sorted(groups).into_iter().filter(|nsg| name.is_none_or(|name| tools::same(tools::text(nsg, "/name"), name))) {
        lines.push(format!(
            "{}  {}  підмережі: {}  NIC: {}",
            tools::text(nsg, "/name"),
            tools::part(tools::text(nsg, "/id"), "resourceGroups"),
            attached(nsg, "subnets"),
            attached(nsg, "networkInterfaces"),
        ));
        lines.push(format!("  відкрито ззовні: {}", open(nsg)));

        let mut rules: Vec<(&str, i64, String)> = Vec::new();
        for (key, default) in [("securityRules", false), ("defaultSecurityRules", true)] {
            if custom && default {
                continue;
            }

            for rule in nsg["properties"][key].as_array().into_iter().flatten() {
                rules.push((
                    rule["properties"]["direction"].as_str().unwrap(),
                    rule["properties"]["priority"].as_i64().unwrap(),
                    line(rule, default),
                ));
            }
        }

        rules.sort_by(|left, right| (left.0, left.1).cmp(&(right.0, right.1)));
        lines.extend(rules.into_iter().map(|rule| rule.2));
    }

    if lines.is_empty() { "Груп безпеки не знайдено.".to_string() } else { lines.join("\n") }
}

pub async fn run(http: &reqwest::Client, arguments: &serde_json::Value) -> serde_json::Value {
    let scope = match tools::scope(arguments).await {
        Ok(scope) => scope,
        Err(message) => return tools::reply(Err(message)),
    };
    let path = format!("{}/providers/Microsoft.Network/networkSecurityGroups?api-version=2022-01-01", scope);
    
    let custom = arguments["custom"].as_bool().unwrap_or(false);
    
    tools::reply(client::list(http, &path).await.map(|groups| table(&groups, arguments["nsg"].as_str(), custom)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<serde_json::Value> {
        vec![serde_json::json!({
            "name": "demo-nsg",
            "id": "/subscriptions/s/resourceGroups/demo-rg/providers/Microsoft.Network/networkSecurityGroups/demo-nsg",
            "properties": {
                "subnets": [{ "id": "/subscriptions/s/resourceGroups/demo-rg/providers/Microsoft.Network/virtualNetworks/demo-vnet/subnets/default" }],
                "securityRules": [{
                    "name": "ssh-web",
                    "properties": {
                        "priority": 300, "direction": "Inbound", "access": "Allow", "protocol": "Tcp",
                        "sourceAddressPrefix": "*", "sourcePortRange": "*",
                        "destinationAddressPrefix": "*", "destinationPortRanges": ["22", "443"],
                    },
                }],
                "defaultSecurityRules": [{
                    "name": "DenyAllInBound",
                    "properties": {
                        "priority": 65500, "direction": "Inbound", "access": "Deny", "protocol": "*",
                        "sourceAddressPrefix": "*", "sourcePortRange": "*",
                        "destinationAddressPrefix": "*", "destinationPortRange": "*",
                    },
                }],
            },
        })]
    }
    
    #[test]
    fn table_lists_rules_by_priority() {
        assert_eq!(
            table(&sample(), None, false),
            "demo-nsg  demo-rg  підмережі: demo-vnet/default  NIC: -\n  відкрито ззовні: 22,443/TCP\n    300 Inbound  Allow TCP  *:* -> *:22,443  ssh-web\n  65500 Inbound  Deny  *    *:* -> *:*  DenyAllInBound (типове)"
        );
    }
    
    #[test]
    fn table_hides_default_rules() {
        assert_eq!(
            table(&sample(), None, true),
            "demo-nsg  demo-rg  підмережі: demo-vnet/default  NIC: -\n  відкрито ззовні: 22,443/TCP\n    300 Inbound  Allow TCP  *:* -> *:22,443  ssh-web"
        );
    }
    
    #[test]
    fn open_ignores_restricted_sources() {
        let nsg = serde_json::json!({
            "properties": { "securityRules": [
                { "name": "vnet", "properties": { "priority": 100, "direction": "Inbound", "access": "Allow", "protocol": "Tcp", "sourceAddressPrefix": "10.0.0.0/24", "destinationPortRange": "3306" } },
                { "name": "any", "properties": { "priority": 110, "direction": "Inbound", "access": "Allow", "protocol": "ICMP", "sourceAddressPrefix": "*", "destinationPortRange": "*" } },
                { "name": "out", "properties": { "priority": 120, "direction": "Outbound", "access": "Allow", "protocol": "Tcp", "sourceAddressPrefix": "*", "destinationPortRange": "443" } },
            ] },
        });
    
        assert_eq!(open(&nsg), "*/ICMP");
        assert_eq!(open(&serde_json::json!({ "properties": { "securityRules": [
            { "name": "low", "properties": { "priority": 100, "direction": "Inbound", "access": "Allow", "protocol": "tcp", "sourceAddressPrefix": "*", "destinationPortRange": "22" } },
        ] } })), "22/TCP");
        assert_eq!(open(&serde_json::json!({ "properties": {} })), "-");
    }
    
    #[test]
    fn table_filters_by_name() {
        assert_eq!(table(&sample(), Some("other"), false), "Груп безпеки не знайдено.");
    }
}
