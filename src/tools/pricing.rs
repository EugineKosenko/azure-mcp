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
fn hourly(items: &[serde_json::Value], windows: bool) -> Option<f64> {
    items
        .iter()
        .filter(|item| !tools::text(item, "/meterName").contains("Spot"))
        .filter(|item| !tools::text(item, "/meterName").contains("Low Priority"))
        .find(|item| tools::text(item, "/productName").contains("Windows") == windows)
        .and_then(|item| item["retailPrice"].as_f64())
}

fn vm(items: &[serde_json::Value], size: &str) -> String {
    if items.is_empty() {
        return format!("VM {}: цін не знайдено (перевірте назву типу, напр. через skus).", size);
    }

    format!(
        "VM {} ({} на годину):\nLinux    {}\nWindows  {}",
        size,
        currency(items),
        hourly(items, false).map_or("-".to_string(), |price| format!("{:.3}", price)),
        hourly(items, true).map_or("-".to_string(), |price| format!("{:.3}", price)),
    )
}

pub async fn run(http: &reqwest::Client, arguments: &serde_json::Value) -> serde_json::Value {
    let Some(region) = arguments["region"].as_str() else {
        return tools::reply(Err("Аргумент region обов'язковий (armRegionName, напр. eastus).".to_string()));
    };
    let filter = format!("armRegionName eq '{}' and productName eq 'Standard HDD Managed Disks'", region);
    let storage = match client::retail(http, &filter).await {
        Ok(items) => items,
        Err(message) => return tools::reply(Err(message)),
    };
    
    let Some(size) = arguments["size"].as_str() else {
        return tools::reply(Ok(table(&storage)));
    };
    let filter = format!("armRegionName eq '{}' and armSkuName eq '{}' and priceType eq 'Consumption'", region, size);
    
    tools::reply(client::retail(http, &filter).await.map(|items| format!("{}\n\n{}", table(&storage), vm(&items, size))))
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
    
    fn vm_items() -> Vec<serde_json::Value> {
        vec![
            serde_json::json!({ "currencyCode": "USD", "retailPrice": 0.138, "meterName": "D4als v6 Low Priority", "productName": "Virtual Machines Dalsv6 Series Windows" }),
            serde_json::json!({ "currencyCode": "USD", "retailPrice": 0.345, "meterName": "D4als v6", "productName": "Virtual Machines Dalsv6 Series Windows" }),
            serde_json::json!({ "currencyCode": "USD", "retailPrice": 0.161, "meterName": "D4als v6", "productName": "Virtual Machines Dalsv6 Series" }),
            serde_json::json!({ "currencyCode": "USD", "retailPrice": 0.032, "meterName": "D4als v6 Spot", "productName": "Virtual Machines Dalsv6 Series" }),
        ]
    }
    
    #[test]
    fn vm_picks_on_demand_linux_and_windows() {
        assert_eq!(
            vm(&vm_items(), "Standard_D4als_v6"),
            "VM Standard_D4als_v6 (USD на годину):\nLinux    0.161\nWindows  0.345"
        );
    }
    
    #[test]
    fn vm_empty_is_a_hint() {
        assert_eq!(vm(&[], "Standard_Unknown"), "VM Standard_Unknown: цін не знайдено (перевірте назву типу, напр. через skus).");
    }
}
