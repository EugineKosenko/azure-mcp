use crate::client;
use crate::tools;

const VERSION: &str = "api-version=2023-04-01";

fn redundancy(config: &serde_json::Value) -> &str {
    match tools::text(config, "/properties/storageModelType") {
        "LocallyRedundant" => "LRS",
        "GeoRedundant" => "GRS",
        "ZoneRedundant" => "ZRS",
        other => other,
    }
}

fn vault(item: &serde_json::Value, full: &serde_json::Value, config: &serde_json::Value, count: usize) -> String {
    format!(
        "{}  {}  {}  {}  storage={}  items={}  immutable={}  softdelete={}({}d,enhanced={})  mua={}  crr={}",
        tools::text(item, "/name"),
        tools::part(tools::text(item, "/id"), "resourceGroups").to_lowercase(),
        tools::text(item, "/location"),
        tools::text(item, "/sku/name"),
        redundancy(config),
        count,
        full.pointer("/properties/securitySettings/immutabilitySettings/state").and_then(|state| state.as_str()).unwrap_or("unreported"),
        tools::text(full, "/properties/securitySettings/softDeleteSettings/softDeleteState"),
        tools::shown(full, "/properties/securitySettings/softDeleteSettings/softDeleteRetentionPeriodInDays"),
        tools::text(full, "/properties/securitySettings/softDeleteSettings/enhancedSecurityState"),
        tools::text(full, "/properties/securitySettings/multiUserAuthorization"),
        tools::text(full, "/properties/redundancySettings/crossRegionRestore"),
    )
}
fn times(schedule: &serde_json::Value) -> String {
    let found: Vec<String> = ["/scheduleRunTimes", "/dailySchedule/scheduleRunTimes", "/weeklySchedule/scheduleRunTimes"]
        .iter()
        .filter_map(|pointer| schedule.pointer(pointer)?.as_array())
        .flatten()
        .map(|time| time.as_str().unwrap().chars().skip(11).take(5).collect())
        .collect();

    if found.is_empty() { "-".to_string() } else { found.join(",") }
}

fn schedule(policy: &serde_json::Value) -> String {
    let base = policy.pointer("/properties/schedulePolicy").unwrap_or(&serde_json::Value::Null);
    let hourly = base.pointer("/hourlySchedule/interval").map(|hours| format!("every {}h", hours));

    format!("{} {}", tools::text(base, "/scheduleRunFrequency"), hourly.unwrap_or_else(|| times(base)))
}

fn duration(policy: &serde_json::Value, pointer: &str) -> Option<String> {
    let count = policy.pointer(&format!("{}/count", pointer))?;
    let unit = tools::text(policy, &format!("{}/durationType", pointer)).chars().next()?.to_ascii_lowercase();

    Some(format!("{}{}", count, unit))
}

fn keep(policy: &serde_json::Value) -> String {
    let base = "/properties/retentionPolicy";
    let levels: Vec<String> = [("daily", "dailySchedule"), ("weekly", "weeklySchedule"), ("monthly", "monthlySchedule"), ("yearly", "yearlySchedule")]
        .iter()
        .filter_map(|(label, key)| Some(format!("{}={}", label, duration(policy, &format!("{}/{}/retentionDuration", base, key))?)))
        .chain(duration(policy, &format!("{}/retentionDuration", base)).map(|value| format!("keep={}", value)))
        .collect();

    if levels.is_empty() { "-".to_string() } else { levels.join(" ") }
}

fn policy(item: &serde_json::Value, count: usize) -> String {
    format!(
        "  policy {}  {}  {}  keep: {}  instant={}  items={}",
        tools::text(item, "/name"),
        tools::text(item, "/properties/backupManagementType"),
        schedule(item),
        keep(item),
        tools::shown(item, "/properties/instantRpRetentionRangeInDays"),
        count,
    )
}
fn item(entry: &serde_json::Value, points: usize) -> String {
    format!(
        "  item {}  {}  policy={}  protection={}  health={}  last={} {}  points={}",
        tools::text(entry, "/properties/friendlyName"),
        tools::text(entry, "/properties/protectedItemType"),
        tools::text(entry, "/properties/policyName"),
        tools::text(entry, "/properties/protectionState"),
        tools::text(entry, "/properties/healthStatus"),
        tools::text(entry, "/properties/lastBackupStatus"),
        tools::text(entry, "/properties/lastBackupTime").chars().take(16).collect::<String>(),
        points,
    )
}
async fn tree(http: &reqwest::Client, one: &serde_json::Value) -> Result<String, String> {
    let id = tools::text(one, "/id");
    let config = client::get(http, &format!("{}/backupstorageconfig/vaultstorageconfig?{}", id, VERSION)).await?;
    let full = client::get(http, &format!("{}?api-version=2024-04-01", id)).await?;
    let policies = client::list(http, &format!("{}/backupPolicies?{}", id, VERSION)).await?;
    let mut entries = client::list(http, &format!("{}/backupProtectedItems?{}", id, VERSION)).await?;
    let mut lines = vec![vault(one, &full, &config, entries.len())];

    for one in tools::sorted(&policies) {
        lines.push(policy(one, entries.iter().filter(|entry| tools::same(tools::text(entry, "/properties/policyName"), tools::text(one, "/name"))).count()));
    }

    entries.sort_by_key(|entry| tools::text(entry, "/properties/friendlyName").to_lowercase());

    for entry in &entries {
        let points = client::list(http, &format!("{}/recoveryPoints?{}", tools::text(entry, "/id"), VERSION)).await?.len();

        lines.push(item(entry, points));
    }

    Ok(lines.join("\n"))
}

pub async fn run(http: &reqwest::Client, arguments: &serde_json::Value) -> serde_json::Value {
    let scope = match tools::scope(arguments).await {
        Ok(scope) => scope,
        Err(message) => return tools::reply(Err(message)),
    };
    let path = format!("{}/providers/Microsoft.RecoveryServices/vaults?api-version=2024-04-01", scope);
    let vaults = match client::list(http, &path).await {
        Ok(vaults) => vaults,
        Err(message) => return tools::reply(Err(message)),
    };
    let name = arguments["name"].as_str();
    let mut blocks = Vec::new();
    
    for one in tools::sorted(&vaults).into_iter().filter(|one| name.is_none_or(|name| tools::same(tools::text(one, "/name"), name))) {
        match tree(http, one).await {
            Ok(block) => blocks.push(block),
            Err(message) => return tools::reply(Err(message)),
        }
    }
    
    tools::reply(Ok(if blocks.is_empty() { "Сховищ не знайдено.".to_string() } else { blocks.join("\n\n") }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn old_policy() -> serde_json::Value {
        serde_json::json!({
            "name": "DailyPolicy",
            "properties": {
                "backupManagementType": "AzureIaasVM",
                "instantRpRetentionRangeInDays": 2,
                "schedulePolicy": { "schedulePolicyType": "SimpleSchedulePolicy", "scheduleRunFrequency": "Daily", "scheduleRunTimes": ["2026-01-01T03:00:00Z"] },
                "retentionPolicy": {
                    "retentionPolicyType": "LongTermRetentionPolicy",
                    "dailySchedule": { "retentionDuration": { "count": 30, "durationType": "Days" } },
                    "weeklySchedule": { "retentionDuration": { "count": 12, "durationType": "Weeks" } },
                    "yearlySchedule": { "retentionDuration": { "count": 5, "durationType": "Years" } },
                },
            },
        })
    }
    
    #[test]
    fn policy_shows_schedule_and_keep() {
        assert_eq!(
            policy(&old_policy(), 3),
            "  policy DailyPolicy  AzureIaasVM  Daily 03:00  keep: daily=30d weekly=12w yearly=5y  instant=2  items=3"
        );
    }
    
    #[test]
    fn policy_new_form_and_simple_keep() {
        let policy_v2 = serde_json::json!({
            "name": "Enhanced",
            "properties": {
                "backupManagementType": "AzureIaasVM",
                "schedulePolicy": { "schedulePolicyType": "SimpleSchedulePolicyV2", "scheduleRunFrequency": "Hourly", "hourlySchedule": { "interval": 4 } },
                "retentionPolicy": { "retentionPolicyType": "SimpleRetentionPolicy", "retentionDuration": { "count": 7, "durationType": "Days" } },
            },
        });
    
        assert_eq!(policy(&policy_v2, 0), "  policy Enhanced  AzureIaasVM  Hourly every 4h  keep: keep=7d  instant=-  items=0");
    }
    
    #[test]
    fn item_shows_last_backup_and_points() {
        let entry = serde_json::json!({
            "properties": {
                "friendlyName": "web-vm",
                "protectedItemType": "Microsoft.Compute/virtualMachines",
                "policyName": "DailyPolicy",
                "protectionState": "Protected",
                "healthStatus": "Passed",
                "lastBackupStatus": "Completed",
                "lastBackupTime": "2026-09-28T03:01:17.123Z",
            },
        });
    
        assert_eq!(
            item(&entry, 12),
            "  item web-vm  Microsoft.Compute/virtualMachines  policy=DailyPolicy  protection=Protected  health=Passed  last=Completed 2026-09-28T03:01  points=12"
        );
    }
    
    #[test]
    fn vault_shows_storage_and_count() {
        let one = serde_json::json!({
            "name": "demo-br-eastus",
            "id": "/subscriptions/s/resourceGroups/DEMO-RG/providers/Microsoft.RecoveryServices/vaults/demo-br-eastus",
            "location": "eastus",
            "sku": { "name": "RS0" },
        });
        let full = serde_json::json!({
            "properties": {
                "redundancySettings": { "crossRegionRestore": "Disabled" },
                "securitySettings": {
                    "immutabilitySettings": { "state": "Locked" },
                    "multiUserAuthorization": "Disabled",
                    "softDeleteSettings": { "softDeleteState": "Enabled", "softDeleteRetentionPeriodInDays": 14, "enhancedSecurityState": "Enabled" },
                },
            },
        });
        let config = serde_json::json!({ "properties": { "storageModelType": "GeoRedundant" } });
    
        assert_eq!(vault(&one, &full, &config, 2), "demo-br-eastus  demo-rg  eastus  RS0  storage=GRS  items=2  immutable=Locked  softdelete=Enabled(14d,enhanced=Enabled)  mua=Disabled  crr=Disabled");
        assert!(vault(&one, &serde_json::json!({}), &config, 0).ends_with("immutable=unreported  softdelete=-(-d,enhanced=-)  mua=-  crr=-"));
        assert_eq!(redundancy(&serde_json::json!({})), "-");
    }
}
