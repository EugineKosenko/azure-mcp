use crate::client;
use crate::tools;

fn table(body: &serde_json::Value) -> String {
    format!(
        "displayName={}\nsubscriptionId={}\nstate={}\ntenantId={}\nquotaId={}\nspendingLimit={}",
        tools::text(body, "/displayName"),
        tools::text(body, "/subscriptionId"),
        tools::text(body, "/state"),
        tools::text(body, "/tenantId"),
        tools::text(body, "/subscriptionPolicies/quotaId"),
        tools::text(body, "/subscriptionPolicies/spendingLimit"),
    )
}

pub async fn run(http: &reqwest::Client, arguments: &serde_json::Value) -> serde_json::Value {
    let subscription = match tools::subscription(arguments).await {
        Ok(subscription) => subscription,
        Err(message) => return tools::reply(Err(message)),
    };
    
    tools::reply(client::get(http, &format!("{}?api-version=2022-12-01", subscription)).await.map(|body| table(&body)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_shows_fields() {
        let body = serde_json::json!({
            "displayName": "Demo Subscription",
            "subscriptionId": "11111111-1111-1111-1111-111111111111",
            "state": "Enabled",
            "tenantId": "22222222-2222-2222-2222-222222222222",
            "subscriptionPolicies": { "quotaId": "PayAsYouGo_2014-09-01", "spendingLimit": "Off" },
        });
    
        assert_eq!(
            table(&body),
            "displayName=Demo Subscription\n\
             subscriptionId=11111111-1111-1111-1111-111111111111\n\
             state=Enabled\n\
             tenantId=22222222-2222-2222-2222-222222222222\n\
             quotaId=PayAsYouGo_2014-09-01\n\
             spendingLimit=Off"
        );
    }
}
