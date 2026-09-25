use crate::client;
use crate::tools;

fn resource(id: &str) -> String {
    if tools::part(id, "resourceGroups") == "-" { "підписка".to_string() } else { tools::last(id).to_string() }
}

fn economy(item: &serde_json::Value) -> String {
    let amount = tools::text(item, "/properties/extendedProperties/annualSavingsAmount");
    let currency = tools::text(item, "/properties/extendedProperties/savingsCurrency");

    if amount == "-" { "-".to_string() } else { format!("{} {}/рік", amount, currency) }
}

fn line(item: &serde_json::Value) -> String {
    let id = tools::text(item, "/properties/resourceMetadata/resourceId");

    format!(
        "{}  {}  {}  {}  {}  {}",
        tools::part(id, "resourceGroups"),
        tools::text(item, "/properties/impact"),
        tools::text(item, "/properties/category"),
        resource(id),
        economy(item),
        tools::text(item, "/properties/shortDescription/problem"),
    )
}

fn table(items: &[serde_json::Value], category: Option<&str>, group: Option<&str>) -> String {
    let lines: Vec<String> = tools::sorted(items)
        .into_iter()
        .filter(|item| category.is_none_or(|category| tools::same(tools::text(item, "/properties/category"), category)))
        .filter(|item| {
            group.is_none_or(|group| {
                tools::same(tools::part(tools::text(item, "/properties/resourceMetadata/resourceId"), "resourceGroups"), group)
            })
        })
        .map(line)
        .collect();

    if lines.is_empty() { "Рекомендацій не знайдено.".to_string() } else { lines.join("\n") }
}

pub async fn run(http: &reqwest::Client, arguments: &serde_json::Value) -> serde_json::Value {
    let subscription = match tools::subscription(arguments).await {
        Ok(subscription) => subscription,
        Err(message) => return tools::reply(Err(message)),
    };
    let path = format!("{}/providers/Microsoft.Advisor/recommendations?api-version=2025-01-01", subscription);
    
    tools::reply(
        client::list(http, &path).await.map(|items| table(&items, arguments["category"].as_str(), arguments["group"].as_str())),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> Vec<serde_json::Value> {
        vec![
            serde_json::json!({
                "id": "/subscriptions/s/resourcegroups/demo-rg/providers/microsoft.compute/virtualmachines/web-vm/providers/Microsoft.Advisor/recommendations/h1",
                "name": "h1",
                "properties": {
                    "category": "Cost",
                    "impact": "High",
                    "shortDescription": { "problem": "Right-size or shutdown underutilized virtual machines" },
                    "resourceMetadata": { "resourceId": "/subscriptions/s/resourcegroups/demo-rg/providers/microsoft.compute/virtualmachines/web-vm" },
                    "extendedProperties": { "annualSavingsAmount": "324", "savingsCurrency": "USD" },
                },
            }),
            serde_json::json!({
                "id": "/subscriptions/s/providers/Microsoft.Advisor/recommendations/h2",
                "name": "h2",
                "properties": {
                    "category": "Cost",
                    "impact": "High",
                    "shortDescription": { "problem": "Consider virtual machine reserved instance" },
                    "resourceMetadata": { "resourceId": "/subscriptions/s" },
                    "extendedProperties": { "annualSavingsAmount": "204", "savingsCurrency": "USD" },
                },
            }),
            serde_json::json!({
                "id": "/subscriptions/s/resourcegroups/demo-rg/providers/microsoft.compute/virtualmachines/web-vm/providers/Microsoft.Advisor/recommendations/h3",
                "name": "h3",
                "properties": {
                    "category": "HighAvailability",
                    "impact": "Medium",
                    "shortDescription": { "problem": "Enable backup" },
                    "resourceMetadata": { "resourceId": "/subscriptions/s/resourcegroups/demo-rg/providers/microsoft.compute/virtualmachines/web-vm" },
                    "extendedProperties": {},
                },
            }),
        ]
    }
    
    #[test]
    fn table_shows_resource_and_economy() {
        let text = table(&items(), None, None);
    
        assert!(text.contains("demo-rg  High  Cost  web-vm  324 USD/рік  Right-size"));
        assert!(text.contains("-  High  Cost  підписка  204 USD/рік  Consider"));
        assert!(text.contains("demo-rg  Medium  HighAvailability  web-vm  -  Enable backup"));
    }
    
    #[test]
    fn table_filters_by_category_and_group() {
        let text = table(&items(), Some("cost"), None);
        assert!(text.contains("Right-size"));
        assert!(text.contains("Consider"));
        assert!(!text.contains("Enable backup"));
    
        assert_eq!(table(&items(), None, Some("other")), "Рекомендацій не знайдено.");
    }
}
