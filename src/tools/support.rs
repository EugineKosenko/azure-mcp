use crate::client;
use crate::tools;

fn line(ticket: &serde_json::Value) -> String {
    format!(
        "{}  {}  {}  {}  {}: {}  {}",
        tools::text(ticket, "/properties/supportTicketId"),
        tools::text(ticket, "/properties/status"),
        tools::text(ticket, "/properties/severity"),
        tools::text(ticket, "/properties/createdDate"),
        tools::text(ticket, "/properties/serviceDisplayName"),
        tools::text(ticket, "/properties/problemClassificationDisplayName"),
        tools::text(ticket, "/properties/title"),
    )
}

fn table(tickets: &[serde_json::Value]) -> String {
    let mut items: Vec<&serde_json::Value> = tickets.iter().collect();
    items.sort_by(|left, right| tools::text(right, "/properties/createdDate").cmp(tools::text(left, "/properties/createdDate")));

    let lines: Vec<String> = items.into_iter().map(line).collect();

    if lines.is_empty() { "Запитів підтримки не знайдено.".to_string() } else { lines.join("\n") }
}

pub async fn run(http: &reqwest::Client, arguments: &serde_json::Value) -> serde_json::Value {
    let subscription = match tools::subscription(arguments).await {
        Ok(subscription) => subscription,
        Err(message) => return tools::reply(Err(message)),
    };
    let path = format!("{}/providers/Microsoft.Support/supportTickets?api-version=2020-04-01", subscription);
    
    tools::reply(client::list(http, &path).await.map(|tickets| table(&tickets)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tickets() -> Vec<serde_json::Value> {
        vec![
            serde_json::json!({
                "properties": {
                    "supportTicketId": "0000000000001",
                    "status": "Open",
                    "severity": "Minimal",
                    "createdDate": "2026-09-20T10:00:00Z",
                    "serviceDisplayName": "Service and subscription limits (quotas)",
                    "problemClassificationDisplayName": "Compute-VM (cores-vCPUs) subscription limit increases",
                    "title": "Quota request for Compute-VM",
                    "contactDetails": { "firstName": "Тест", "lastName": "Тестовий", "primaryEmailAddress": "test@example.com" },
                },
            }),
            serde_json::json!({
                "properties": {
                    "supportTicketId": "0000000000002",
                    "status": "Closed",
                    "severity": "Moderate",
                    "createdDate": "2026-09-25T10:00:00Z",
                    "serviceDisplayName": "Virtual machine running Windows",
                    "problemClassificationDisplayName": "Performance",
                    "title": "VM slow",
                },
            }),
        ]
    }
    
    #[test]
    fn table_sorts_newest_first_and_hides_contact() {
        let text = table(&tickets());
    
        assert!(text.starts_with("0000000000002"));
        assert!(!text.contains("Тест"));
        assert!(!text.contains("example.com"));
    }
    
    #[test]
    fn table_empty_is_a_hint() {
        assert_eq!(table(&[]), "Запитів підтримки не знайдено.".to_string());
    }
}
