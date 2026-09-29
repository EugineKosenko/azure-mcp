use crate::client;
use crate::tools;

fn day(value: &serde_json::Value, pointer: &str) -> String {
    tools::text(value, pointer).chars().take(10).collect()
}

fn regions(version: &serde_json::Value) -> String {
    let items = version.pointer("/properties/publishingProfile/targetRegions").and_then(|item| item.as_array());

    match items {
        Some(items) if !items.is_empty() => items
            .iter()
            .map(|item| format!("{}({},{})", tools::text(item, "/name"), tools::shown(item, "/regionalReplicaCount"), tools::text(item, "/storageAccountType")))
            .collect::<Vec<_>>()
            .join(","),
        _ => "-".to_string(),
    }
}

fn origin(version: &serde_json::Value) -> String {
    ["/properties/storageProfile/source/id", "/properties/storageProfile/osDiskImage/source/id", "/properties/storageProfile/osDiskImage/source/uri"]
        .iter()
        .find(|pointer| tools::text(version, pointer) != "-")
        .map(|pointer| tools::last(tools::text(version, pointer)).to_string())
        .unwrap_or("-".to_string())
}

fn gallery(item: &serde_json::Value) -> String {
    format!(
        "{}  {}  {}  {}",
        tools::text(item, "/name"),
        tools::part(tools::text(item, "/id"), "resourceGroups").to_lowercase(),
        tools::text(item, "/location"),
        tools::text(item, "/properties/provisioningState"),
    )
}

fn definition(item: &serde_json::Value) -> String {
    format!(
        "  {}  {}  {}  {}  {}/{}/{}",
        tools::text(item, "/name"),
        tools::text(item, "/properties/osType"),
        tools::text(item, "/properties/hyperVGeneration"),
        tools::text(item, "/properties/osState"),
        tools::text(item, "/properties/identifier/publisher"),
        tools::text(item, "/properties/identifier/offer"),
        tools::text(item, "/properties/identifier/sku"),
    )
}

fn version(item: &serde_json::Value) -> String {
    let excluded = if item.pointer("/properties/publishingProfile/excludeFromLatest").and_then(|flag| flag.as_bool()) == Some(true) { "  excluded" } else { "" };

    format!(
        "    {}  {} ГБ  published={}  endOfLife={}{}  regions={}  source={}  {}",
        tools::text(item, "/name"),
        tools::shown(item, "/properties/storageProfile/osDiskImage/sizeInGB"),
        day(item, "/properties/publishingProfile/publishedDate"),
        day(item, "/properties/publishingProfile/endOfLifeDate"),
        excluded,
        regions(item),
        origin(item),
        tools::text(item, "/properties/provisioningState"),
    )
}
const VERSION: &str = "api-version=2024-03-03";

async fn children(http: &reqwest::Client, parent: &serde_json::Value, kind: &str) -> Result<Vec<serde_json::Value>, String> {
    client::list(http, &format!("{}/{}?{}", tools::text(parent, "/id"), kind, VERSION)).await
}

async fn tree(http: &reqwest::Client, item: &serde_json::Value) -> Result<String, String> {
    let mut lines = vec![gallery(item)];

    for one in tools::sorted(&children(http, item, "images").await?) {
        lines.push(definition(one));

        let mut versions = children(http, one, "versions").await?;

        versions.sort_by_key(|item| std::cmp::Reverse(tools::text(item, "/name").to_string()));
        lines.extend(versions.iter().map(version));
    }

    Ok(lines.join("\n"))
}

pub async fn run(http: &reqwest::Client, arguments: &serde_json::Value) -> serde_json::Value {
    let scope = match tools::scope(arguments).await {
        Ok(scope) => scope,
        Err(message) => return tools::reply(Err(message)),
    };
    let galleries = match client::list(http, &format!("{}/providers/Microsoft.Compute/galleries?{}", scope, VERSION)).await {
        Ok(galleries) => galleries,
        Err(message) => return tools::reply(Err(message)),
    };
    let name = arguments["name"].as_str();
    let mut blocks = Vec::new();
    
    for item in tools::sorted(&galleries).into_iter().filter(|item| name.is_none_or(|name| tools::same(tools::text(item, "/name"), name))) {
        match tree(http, item).await {
            Ok(block) => blocks.push(block),
            Err(message) => return tools::reply(Err(message)),
        }
    }
    
    tools::reply(Ok(if blocks.is_empty() { "Галерей не знайдено.".to_string() } else { blocks.join("\n\n") }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn versions() -> serde_json::Value {
        serde_json::json!({
            "name": "1.0.0",
            "properties": {
                "provisioningState": "Succeeded",
                "publishingProfile": {
                    "publishedDate": "2026-09-01T10:00:00+00:00",
                    "excludeFromLatest": true,
                    "targetRegions": [
                        { "name": "East US", "regionalReplicaCount": 1, "storageAccountType": "Standard_LRS" },
                        { "name": "Canada Central", "regionalReplicaCount": 2, "storageAccountType": "Standard_ZRS" },
                    ],
                },
                "storageProfile": { "osDiskImage": { "sizeInGB": 30 }, "source": { "id": "/subscriptions/s/resourceGroups/demo-rg/providers/Microsoft.Compute/images/web-image" } },
            },
        })
    }
    
    #[test]
    fn version_shows_regions_and_source() {
        assert_eq!(
            version(&versions()),
            "    1.0.0  30 ГБ  published=2026-09-01  endOfLife=-  excluded  regions=East US(1,Standard_LRS),Canada Central(2,Standard_ZRS)  source=web-image  Succeeded"
        );
    }
    
    #[test]
    fn version_without_optional_fields() {
        let bare = serde_json::json!({ "name": "2.0.0", "properties": { "provisioningState": "Creating" } });
    
        assert_eq!(version(&bare), "    2.0.0  - ГБ  published=-  endOfLife=-  regions=-  source=-  Creating");
    }
    
    #[test]
    fn gallery_and_definition_lines() {
        let one = serde_json::json!({
            "name": "demo_gallery",
            "id": "/subscriptions/s/resourceGroups/DEMO-RG/providers/Microsoft.Compute/galleries/demo_gallery",
            "location": "eastus",
            "properties": { "provisioningState": "Succeeded" },
        });
        let image = serde_json::json!({
            "name": "web",
            "properties": { "osType": "Linux", "osState": "Generalized", "hyperVGeneration": "V2", "identifier": { "publisher": "demo", "offer": "debian", "sku": "12" } },
        });
    
        assert_eq!(gallery(&one), "demo_gallery  demo-rg  eastus  Succeeded");
        assert_eq!(definition(&image), "  web  Linux  V2  Generalized  demo/debian/12");
    }
}
