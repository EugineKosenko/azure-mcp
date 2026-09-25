use crate::client;
use crate::tools;

const TIERS: [(&str, u32); 11] = [
    ("S4", 32), ("S6", 64), ("S10", 128), ("S15", 256), ("S20", 512),
    ("S30", 1024), ("S40", 2048), ("S50", 4096), ("S60", 8192), ("S70", 16384), ("S80", 32767),
];
fn currency(items: &[serde_json::Value]) -> &str {
    items.first().map(|item| tools::text(item, "/currencyCode")).unwrap_or("-")
}

fn snapshots(items: &[serde_json::Value]) -> String {
    items
        .iter()
        .filter(|item| tools::text(item, "/meterName").ends_with(" Snapshots"))
        .map(|item| {
            format!(
                "{:>8.3}  {}",
                item["retailPrice"].as_f64().unwrap(),
                tools::text(item, "/meterName").trim_end_matches(" Snapshots"),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn tiers(items: &[serde_json::Value]) -> String {
    TIERS
        .iter()
        .filter_map(|(tier, gib)| {
            items
                .iter()
                .find(|item| tools::text(item, "/meterName") == format!("{} LRS Disk", tier))
                .map(|item| format!("{:>8.3}  {}  {} ГБ", item["retailPrice"].as_f64().unwrap(), tier, gib))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn table(items: &[serde_json::Value]) -> String {
    if items.is_empty() {
        return "Цін не знайдено (перевірте код регіону).".to_string();
    }

    let currency = currency(items);

    format!(
        "Снепшоти (за фактично використаний ГБ, {currency} на місяць):\n{}\n\n\
         Рівні дисків (за виділений розмір, округлення вгору, {currency} на місяць):\n{}\n\n\
         Образ (Microsoft.Compute/images) Azure окремо не документує: можливо, тарифікується як диск за \
         рівнем (друга таблиця), можливо — як снепшот за використаними даними (перша); точну модель \
         перевірте на першому реальному образі інструментом costs.",
        snapshots(items), tiers(items),
    )
}

pub async fn run(http: &reqwest::Client, arguments: &serde_json::Value) -> serde_json::Value {
    let Some(region) = arguments["region"].as_str() else {
        return tools::reply(Err("Аргумент region обов'язковий (armRegionName, напр. eastus).".to_string()));
    };
    let filter = format!("armRegionName eq '{}' and productName eq 'Standard HDD Managed Disks'", region);
    
    tools::reply(client::retail(http, &filter).await.map(|items| table(&items)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> Vec<serde_json::Value> {
        vec![
            serde_json::json!({ "currencyCode": "USD", "retailPrice": 0.05, "meterName": "LRS Snapshots" }),
            serde_json::json!({ "currencyCode": "USD", "retailPrice": 0.05, "meterName": "ZRS Snapshots" }),
            serde_json::json!({ "currencyCode": "USD", "retailPrice": 1.536, "meterName": "S4 LRS Disk" }),
            serde_json::json!({ "currencyCode": "USD", "retailPrice": 0.0005, "meterName": "S4 LRS Disk Operations" }),
            serde_json::json!({ "currencyCode": "USD", "retailPrice": 3.008, "meterName": "S6 LRS Disk" }),
        ]
    }
    
    #[test]
    fn table_separates_snapshots_and_tiers() {
        let text = table(&items());
    
        assert!(text.contains("0.050  LRS"));
        assert!(text.contains("0.050  ZRS"));
        assert!(text.contains("1.536  S4  32 ГБ"));
        assert!(text.contains("3.008  S6  64 ГБ"));
        assert!(!text.contains("Operations"));
    }
    
    #[test]
    fn table_empty_is_a_hint() {
        assert_eq!(table(&[]), "Цін не знайдено (перевірте код регіону).".to_string());
    }
}
