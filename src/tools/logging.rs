use crate::client;
use crate::tools;

fn related(name: &str) -> bool {
    ["log", "audit", "performance_schema", "min_examined_row_limit", "long_query_time"].iter().any(|key| name.contains(key))
}

fn setting(item: &serde_json::Value) -> String {
    let name = tools::text(item, "/name");
    let value = tools::shown(item, "/properties/value");

    if tools::same(tools::text(item, "/properties/source"), "user-override") {
        format!("  {}={} *(за замовчуванням {})", name, value, tools::shown(item, "/properties/defaultValue"))
    } else {
        format!("  {}={}", name, value)
    }
}
fn destinations(item: &serde_json::Value) -> String {
    let found: Vec<String> = [("workspace", "/properties/workspaceId"), ("storage", "/properties/storageAccountId"), ("eventhub", "/properties/eventHubName")]
        .iter()
        .filter_map(|(label, pointer)| item.pointer(pointer).and_then(|value| value.as_str()).filter(|value| !value.is_empty()).map(|value| format!("{}={}", label, tools::last(value))))
        .collect();

    if found.is_empty() { "-".to_string() } else { found.join(" ") }
}

fn categories(item: &serde_json::Value) -> String {
    let found: Vec<&str> = item["properties"]["logs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|log| log["enabled"] == true)
        .filter_map(|log| log["category"].as_str().or(log["categoryGroup"].as_str()))
        .collect();

    if found.is_empty() { "-".to_string() } else { found.join(", ") }
}

fn diagnostics(items: &[serde_json::Value]) -> Vec<String> {
    if items.is_empty() {
        return vec!["  diagnostic settings немає: журнали ніде не збираються".to_string()];
    }

    items.iter().map(|item| format!("  diagnostic {}: {}  категорії: {}", tools::text(item, "/name"), destinations(item), categories(item))).collect()
}
fn block(server: &serde_json::Value, configs: &[serde_json::Value], settings: &[serde_json::Value]) -> String {
    let mut lines = vec![format!("{}  {}", tools::text(server, "/name"), tools::part(tools::text(server, "/id"), "resourceGroups"))];
    let shown: Vec<String> = configs.iter().filter(|item| related(tools::text(item, "/name"))).map(setting).collect();

    lines.push("  параметри журналів:".to_string());
    lines.extend(if shown.is_empty() { vec!["  -".to_string()] } else { shown });
    lines.extend(diagnostics(settings));
    lines.join("\n")
}

pub async fn run(http: &reqwest::Client, arguments: &serde_json::Value) -> serde_json::Value {
    if let Err(message) = tools::unknown(arguments, &["server", "group", "sbscrptn"]) {
        return tools::reply(Err(message));
    }
    
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
    
    let mut blocks = Vec::new();
    for server in tools::sorted(&servers).into_iter().filter(|server| name.is_none_or(|name| tools::same(tools::text(server, "/name"), name))) {
        let id = tools::text(server, "/id");
        let configs = match client::list(http, &format!("{}/configurations?api-version=2024-10-01-preview", id)).await {
            Ok(configs) => configs,
            Err(message) => return tools::reply(Err(message)),
        };
        let settings = match client::list(http, &format!("{}/providers/Microsoft.Insights/diagnosticSettings?api-version=2021-05-01-preview", id)).await {
            Ok(settings) => settings,
            Err(message) => return tools::reply(Err(message)),
        };
    
        blocks.push(block(server, &configs, &settings));
    }
    
    tools::reply(Ok(if blocks.is_empty() { "Серверів MySQL не знайдено.".to_string() } else { blocks.join("\n\n") }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> serde_json::Value {
        serde_json::json!({ "name": "demo-db", "id": "/subscriptions/s/resourceGroups/demo-rg/providers/Microsoft.DBforMySQL/flexibleServers/demo-db" })
    }
    
    fn config(name: &str, value: &str, source: &str, default: &str) -> serde_json::Value {
        serde_json::json!({ "name": name, "properties": { "value": value, "source": source, "defaultValue": default } })
    }
    
    #[test]
    fn related_filters_names() {
        assert!(related("slow_query_log"));
        assert!(related("long_query_time"));
        assert!(related("log_output"));
        assert!(related("audit_log_enabled"));
        assert!(related("performance_schema"));
        assert!(related("min_examined_row_limit"));
        assert!(!related("max_connections"));
    }
    
    #[test]
    fn setting_marks_overrides() {
        assert_eq!(setting(&config("slow_query_log", "ON", "user-override", "OFF")), "  slow_query_log=ON *(за замовчуванням OFF)");
        assert_eq!(setting(&config("log_output", "FILE", "system-default", "FILE")), "  log_output=FILE");
    }
    
    #[test]
    fn block_without_diagnostics_says_so() {
        let configs = vec![config("slow_query_log", "OFF", "system-default", "OFF"), config("max_connections", "100", "system-default", "100")];
    
        assert_eq!(
            block(&server(), &configs, &[]),
            "demo-db  demo-rg\n  параметри журналів:\n  slow_query_log=OFF\n  diagnostic settings немає: журнали ніде не збираються"
        );
    }
    
    #[test]
    fn block_shows_destinations_and_categories() {
        let settings = vec![serde_json::json!({
            "name": "to-logs",
            "properties": {
                "workspaceId": "/subscriptions/s/resourceGroups/demo-rg/providers/Microsoft.OperationalInsights/workspaces/demo-logs",
                "logs": [
                    { "category": "MySqlSlowLogs", "enabled": true },
                    { "category": "MySqlAuditLogs", "enabled": false },
                    { "categoryGroup": "allLogs", "enabled": true },
                ],
            },
        })];
    
        assert_eq!(
            block(&server(), &[], &settings),
            "demo-db  demo-rg\n  параметри журналів:\n  -\n  diagnostic to-logs: workspace=demo-logs  категорії: MySqlSlowLogs, allLogs"
        );
    }
}
