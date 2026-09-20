use crate::client;
mod costs;
mod pubips;
mod nics;
mod nsgrules;
mod firewall;
mod dnszones;
mod vms;
mod disks;

pub fn list() -> serde_json::Value {
    serde_json::json!([
        {
            "name": "costs",
            "description": "Фактичні витрати (ActualCost) підписки Azure чи однієї групи ресурсів за період, з розбивкою за групами ресурсів, ресурсами (група, тип, ім'я), типами ресурсів, категоріями лічильників або службами; від більших до менших, з підсумком. Із daily=true — витрати за кожну добу й середня за повну добу. Остання доба в даних завжди неповна (Cost Management відстає приблизно на півдоби). Дані Cost Management мають спільний ліміт запитів: при 429 сервер повторює запит із паузами, відповідь кешується на годину (у відповіді вказано, коли вона з кешу).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "group": { "type": "string", "description": "Група ресурсів; без неї — уся підписка" },
                    "sbscrptn": { "type": "string", "description": "Ідентифікатор підписки; без нього — AZURE_SBSCRPTN чи підписка за замовчуванням az" },
                    "period": { "type": "string", "enum": ["MonthToDate", "BillingMonthToDate", "TheLastMonth", "TheLastBillingMonth", "WeekToDate"], "description": "Період, за замовчуванням MonthToDate (з початку місяця)" },
                    "from": { "type": "string", "description": "Початок довільного періоду, РРРР-ММ-ДД (разом із to; перекриває period)" },
                    "to": { "type": "string", "description": "Кінець довільного періоду включно, РРРР-ММ-ДД (разом із from)" },
                    "daily": { "type": "boolean", "description": "Витрати за кожну добу й середня за повну добу для кожної групи" },
                    "by": { "type": "string", "enum": ["ResourceGroupName", "ResourceId", "ResourceType", "MeterCategory", "ServiceName"], "description": "Розбивка, за замовчуванням ResourceGroupName; ResourceId показано як «група  тип  ім'я»" },
                    "refresh": { "type": "boolean", "description": "Пропустити кеш і запитати Azure наново" }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "pubips",
            "description": "Публічні IP-адреси Azure: ім'я, група, адреса, SKU і спосіб виділення (Static / Dynamic), власник (NIC чи NAT-шлюз) і ім'я VM. Непривʼязані адреси позначено «не прив'язана»: статична адреса без власника тарифікується. Необов'язковий addr лишає лише вказану адресу.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "addr": { "type": "string", "description": "Лише ця публічна адреса, напр. 203.0.113.10" },
                    "group": { "type": "string", "description": "Група ресурсів; без неї — уся підписка" },
                    "sbscrptn": { "type": "string", "description": "Ідентифікатор підписки; без нього — AZURE_SBSCRPTN чи підписка за замовчуванням az" }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "nics",
            "description": "Мережеві інтерфейси Azure: VM, NSG інтерфейсу, enableIPForwarding, а для кожної IP-конфігурації — приватна адреса, мережа/підмережа і публічна IP. Необов'язковий name лишає один NIC.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Лише NIC з цим іменем" },
                    "group": { "type": "string", "description": "Група ресурсів; без неї — уся підписка" },
                    "sbscrptn": { "type": "string", "description": "Ідентифікатор підписки; без нього — AZURE_SBSCRPTN чи підписка за замовчуванням az" }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "nsgrules",
            "description": "Групи безпеки мережі (NSG) Azure: до яких підмереж і NIC прив'язана, підсумок відкритих із будь-якого джерела портів (без урахування пріоритетів) та її правила за напрямком і пріоритетом. З custom=true типові правила (65000–65500) приховано. Необов'язковий nsg лишає одну групу.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "nsg": { "type": "string", "description": "Лише NSG з цим іменем" },
                    "custom": { "type": "boolean", "description": "Сховати типові правила (65000–65500), лишити власні" },
                    "group": { "type": "string", "description": "Група ресурсів; без неї — уся підписка" },
                    "sbscrptn": { "type": "string", "description": "Ідентифікатор підписки; без нього — AZURE_SBSCRPTN чи підписка за замовчуванням az" }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "firewall",
            "description": "Сервери MySQL Flexible Server: publicNetworkAccess, приватні ендпоінти та правила firewall (ім'я, діапазон адрес) з позначкою для кожного: «усі служби Azure» (0.0.0.0), «приватна мережа», публічні IP підписки, які діапазон покриває, або «не збігається з жодною публічною IP підписки». Необов'язковий server лишає один сервер.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "server": { "type": "string", "description": "Лише сервер з цим іменем" },
                    "group": { "type": "string", "description": "Група ресурсів; без неї — уся підписка" },
                    "sbscrptn": { "type": "string", "description": "Ідентифікатор підписки; без нього — AZURE_SBSCRPTN чи підписка за замовчуванням az" }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "dnszones",
            "description": "Приватні DNS-зони Azure: зв'язки з віртуальними мережами (virtualNetworkLinks) і A-записи. Необов'язковий zone лишає одну зону.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "zone": { "type": "string", "description": "Лише зона з цим іменем, напр. privatelink.mysql.database.azure.com" },
                    "group": { "type": "string", "description": "Група ресурсів; без неї — уся підписка" },
                    "sbscrptn": { "type": "string", "description": "Ідентифікатор підписки; без нього — AZURE_SBSCRPTN чи підписка за замовчуванням az" }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "vms",
            "description": "Віртуальні машини Azure: ім'я, група, розмір, ОС, стан живлення (running / deallocated тощо), приватні й публічні IP. Необов'язковий name лишає одну VM.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Лише VM з цим іменем" },
                    "group": { "type": "string", "description": "Група ресурсів; без неї — уся підписка" },
                    "sbscrptn": { "type": "string", "description": "Ідентифікатор підписки; без нього — AZURE_SBSCRPTN чи підписка за замовчуванням az" }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "disks",
            "description": "Керовані диски Azure: ім'я, група, SKU, розмір у ГБ, стан і VM, до якої диск прив'язано. Непривʼязані диски позначено «не прив'язаний»: вони тарифікуються. Необов'язковий name лишає один диск.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Лише диск з цим іменем" },
                    "group": { "type": "string", "description": "Група ресурсів; без неї — уся підписка" },
                    "sbscrptn": { "type": "string", "description": "Ідентифікатор підписки; без нього — AZURE_SBSCRPTN чи підписка за замовчуванням az" }
                },
                "additionalProperties": false
            }
        }
    ])
}

pub async fn call(http: &reqwest::Client, name: &str, arguments: &serde_json::Value) -> serde_json::Value {
    match name {
        "costs" => costs::run(http, arguments).await,
        "pubips" => pubips::run(http, arguments).await,
        "nics" => nics::run(http, arguments).await,
        "nsgrules" => nsgrules::run(http, arguments).await,
        "firewall" => firewall::run(http, arguments).await,
        "dnszones" => dnszones::run(http, arguments).await,
        "vms" => vms::run(http, arguments).await,
        "disks" => disks::run(http, arguments).await,
        _ => reply(Err(format!("Невідомий інструмент: {}", name)))
    }
}

pub async fn subscription(arguments: &serde_json::Value) -> Result<String, String> {
    let sbscrptn = match arguments["sbscrptn"].as_str() {
        Some(id) => id.to_string(),
        None => client::sbscrptn().await?,
    };

    Ok(format!("/subscriptions/{}", sbscrptn))
}

pub async fn scope(arguments: &serde_json::Value) -> Result<String, String> {
    let subscription = subscription(arguments).await?;

    Ok(match arguments["group"].as_str() {
        Some(group) => format!("{}/resourceGroups/{}", subscription, group),
        None => subscription,
    })
}
pub fn refresh(arguments: &serde_json::Value) -> bool {
    arguments["refresh"].as_bool().unwrap_or(false)
}
pub fn part<'a>(id: &'a str, key: &str) -> &'a str {
    id.split('/').skip_while(|item| !item.eq_ignore_ascii_case(key)).nth(1).unwrap_or("-")
}

pub fn last(id: &str) -> &str {
    id.rsplit('/').next().unwrap()
}

pub fn text<'a>(value: &'a serde_json::Value, pointer: &str) -> &'a str {
    value.pointer(pointer).and_then(|item| item.as_str()).unwrap_or("-")
}

pub fn shown(value: &serde_json::Value, pointer: &str) -> String {
    match value.pointer(pointer) {
        Some(serde_json::Value::String(item)) => item.clone(),
        Some(item) => item.to_string(),
        None => "-".to_string(),
    }
}

pub fn same(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}
pub fn sorted(items: &[serde_json::Value]) -> Vec<&serde_json::Value> {
    let mut items: Vec<&serde_json::Value> = items.iter().collect();

    items.sort_by_key(|item| (part(text(item, "/id"), "resourceGroups").to_lowercase(), text(item, "/name").to_lowercase()));
    items
}
pub fn reply(result: Result<String, String>) -> serde_json::Value {
    match result {
        Ok(text) => serde_json::json!({ "content": [{ "type": "text", "text": text }], "isError": false }),
        Err(text) => serde_json::json!({ "content": [{ "type": "text", "text": text }], "isError": true }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_marks_errors() {
        assert_eq!(reply(Ok("є".to_string()))["isError"], false);
        assert_eq!(reply(Err("немає".to_string()))["isError"], true);
        assert_eq!(reply(Err("немає".to_string()))["content"][0]["text"], "немає");
    }
    
    #[test]
    fn part_ignores_case() {
        let id = "/subscriptions/s/resourcegroups/demo-rg/providers/Microsoft.Network/networkInterfaces/nic1/ipConfigurations/ipconfig1";
    
        assert_eq!(part(id, "resourceGroups"), "demo-rg");
        assert_eq!(part(id, "networkInterfaces"), "nic1");
        assert_eq!(part(id, "subnets"), "-");
        assert_eq!(last(id), "ipconfig1");
    }
    
    #[test]
    fn sorted_by_group_then_name() {
        let items = vec![
            serde_json::json!({ "name": "b", "id": "/subscriptions/s/resourceGroups/TRIAL/providers/p/b" }),
            serde_json::json!({ "name": "Z", "id": "/subscriptions/s/resourceGroups/dev/providers/p/Z" }),
            serde_json::json!({ "name": "a", "id": "/subscriptions/s/resourcegroups/dev/providers/p/a" }),
        ];
        let names: Vec<&str> = sorted(&items).iter().map(|item| text(item, "/name")).collect();
    
        assert_eq!(names, vec!["a", "Z", "b"]);
    }
    
    #[test]
    fn text_missing_is_dash() {
        let value = serde_json::json!({ "properties": { "ipAddress": "10.0.0.9" } });
    
        assert_eq!(text(&value, "/properties/ipAddress"), "10.0.0.9");
        assert_eq!(text(&value, "/properties/ipConfiguration/id"), "-");
    }
    
    #[test]
    fn refresh_defaults_to_false() {
        assert!(!refresh(&serde_json::json!({})));
        assert!(refresh(&serde_json::json!({ "refresh": true })));
    }
}
