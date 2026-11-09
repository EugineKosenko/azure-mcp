use crate::client;
use crate::tools;

fn bound(text: &str, is_end: bool) -> Result<String, String> {
    let digits = |range: std::ops::Range<usize>| text.get(range).is_some_and(|part| part.bytes().all(|byte| byte.is_ascii_digit()));
    let dated = digits(0..4) && text.get(4..5) == Some("-") && digits(5..7) && text.get(7..8) == Some("-") && digits(8..10);

    match text.len() {
        10 if dated => Ok(format!("{}{}", text, if is_end { "T23:59:59Z" } else { "T00:00:00Z" })),
        20 if dated && text.get(10..11) == Some("T") && text.ends_with('Z') => Ok(text.to_string()),
        _ => Err(format!("Хибна дата {}: потрібен формат РРРР-ММ-ДД або РРРР-ММ-ДДTГГ:ХХ:ССZ (UTC).", text)),
    }
}
fn filter(start: &str, end: Option<&str>, resource: Option<&str>, group: Option<&str>) -> String {
    let quoted = |text: &str| text.replace('\'', "''");
    let mut items = vec![format!("eventTimestamp ge '{}'", start)];

    if let Some(end) = end {
        items.push(format!("eventTimestamp le '{}'", end));
    }
    if let Some(id) = resource {
        items.push(format!("resourceUri eq '{}'", quoted(id)));
    } else if let Some(group) = group {
        items.push(format!("resourceGroupName eq '{}'", quoted(group)));
    }

    items.join(" and ")
}
fn is_progress(event: &serde_json::Value) -> bool {
    ["Started", "Accepted", "In Progress"].iter().any(|state| tools::same(tools::text(event, "/status/value"), state))
}

fn row(event: &serde_json::Value) -> String {
    let id = tools::text(event, "/resourceId");
    let operation = event.pointer("/operationName/localizedValue").and_then(|item| item.as_str()).unwrap_or_else(|| tools::text(event, "/operationName/value"));

    format!(
        "{}  {}  {}  {}  {}  {}",
        tools::text(event, "/eventTimestamp"),
        tools::text(event, "/caller"),
        operation,
        tools::text(event, "/status/value"),
        tools::part(id, "resourceGroups"),
        tools::last(id),
    )
}

pub async fn run(http: &reqwest::Client, arguments: &serde_json::Value) -> serde_json::Value {
    if let Err(message) = tools::unknown(arguments, &["resource", "name", "type", "group", "sbscrptn", "start", "end", "all", "limit"]) {
        return tools::reply(Err(message));
    }
    
    let Some(start) = arguments["start"].as_str() else {
        return tools::reply(Err("Вкажіть start (дата РРРР-ММ-ДД або момент ISO 8601 UTC).".to_string()));
    };
    let (start, end) = match (bound(start, false), arguments["end"].as_str().map(|end| bound(end, true)).transpose()) {
        (Ok(start), Ok(end)) => (start, end),
        (Err(message), _) | (_, Err(message)) => return tools::reply(Err(message)),
    };
    let resource = if arguments["resource"].is_null() && arguments["name"].is_null() {
        None
    } else {
        match tools::locate(http, arguments).await {
            Ok(id) => Some(id),
            Err(message) => return tools::reply(Err(message)),
        }
    };
    let subscription = match tools::subscription(arguments).await {
        Ok(subscription) => subscription,
        Err(message) => return tools::reply(Err(message)),
    };
    let path = format!(
        "{}/providers/Microsoft.Insights/eventtypes/management/values?api-version=2015-04-01&$filter={}",
        subscription,
        filter(&start, end.as_deref(), resource.as_deref(), arguments["group"].as_str()),
    );
    let (all, limit) = (arguments["all"].as_bool().unwrap_or(false), arguments["limit"].as_u64().unwrap_or(200) as usize);
    
    tools::reply(client::list(http, &path).await.map(|events| {
        let mut events: Vec<&serde_json::Value> = events.iter().filter(|event| all || !is_progress(event)).collect();
    
        events.sort_by_key(|event| tools::text(event, "/eventTimestamp"));
    
        if events.is_empty() {
            return "Подій за цей період немає.".to_string();
        }
    
        let mut lines: Vec<String> = events.iter().take(limit).map(|event| row(event)).collect();
    
        if events.len() > limit {
            lines.push(format!("Показано {} із {} подій (limit; решту — звузьте період чи область).", limit, events.len()));
        }
    
        lines.join("\n")
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bound_expands_dates() {
        assert_eq!(bound("2026-10-31", false).unwrap(), "2026-10-31T00:00:00Z");
        assert_eq!(bound("2026-11-02", true).unwrap(), "2026-11-02T23:59:59Z");
        assert_eq!(bound("2026-11-01T08:30:00Z", true).unwrap(), "2026-11-01T08:30:00Z");
    }
    
    #[test]
    fn bound_rejects_bad_format() {
        assert!(bound("2026-1-2", false).is_err());
        assert!(bound("01.11.2026", false).is_err());
        assert!(bound("2026-11-01T08:30:00", false).is_err());
    }
    
    #[test]
    fn filter_by_scope() {
        assert_eq!(filter("A", None, None, None), "eventTimestamp ge 'A'");
        assert_eq!(filter("A", Some("B"), None, Some("demo-rg")), "eventTimestamp ge 'A' and eventTimestamp le 'B' and resourceGroupName eq 'demo-rg'");
        assert_eq!(filter("A", None, Some("/x/it's"), Some("demo-rg")), "eventTimestamp ge 'A' and resourceUri eq '/x/it''s'");
    }
    
    #[test]
    fn row_shows_who_and_what() {
        let event = serde_json::json!({
            "eventTimestamp": "2026-11-01T08:30:00.123Z",
            "caller": "user@example.com",
            "operationName": { "value": "Microsoft.DBforMySQL/flexibleServers/write", "localizedValue": "Update server" },
            "status": { "value": "Succeeded" },
            "resourceId": "/subscriptions/s/resourceGroups/demo-rg/providers/Microsoft.DBforMySQL/flexibleServers/demo-db",
        });
    
        assert_eq!(row(&event), "2026-11-01T08:30:00.123Z  user@example.com  Update server  Succeeded  demo-rg  demo-db");
    }
    
    #[test]
    fn progress_states_are_skipped() {
        assert!(is_progress(&serde_json::json!({ "status": { "value": "Started" } })));
        assert!(is_progress(&serde_json::json!({ "status": { "value": "Accepted" } })));
        assert!(!is_progress(&serde_json::json!({ "status": { "value": "Succeeded" } })));
        assert!(!is_progress(&serde_json::json!({ "status": { "value": "Failed" } })));
    }
}
