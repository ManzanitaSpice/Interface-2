use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogItem {
    pub id: String,
    pub source: String,
    pub title: String,
    pub description: String,
    pub image: String,
    pub author: String,
    pub downloads: u64,
    pub updated_at: String,
    pub minecraft_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub project_type: String,
    pub tags: Vec<String>,
}

#[tauri::command]
pub async fn fetch_modrinth_catalog(
    query: String,
    project_type: Option<String>,
    sort_index: String,
    mc_version: Option<String>,
    loader: Option<String>,
) -> Result<Vec<CatalogItem>, String> {
    let mut facets: Vec<Vec<String>> = Vec::new();
    if let Some(value) = project_type.filter(|v| !v.trim().is_empty()) {
        facets.push(vec![format!("project_type:{}", value)]);
    }
    if let Some(value) = mc_version.filter(|v| !v.trim().is_empty()) {
        facets.push(vec![format!("versions:{}", value)]);
    }
    if let Some(value) = loader.filter(|v| !v.trim().is_empty()) {
        facets.push(vec![format!("categories:{}", value.to_ascii_lowercase())]);
    }

    let client = reqwest::Client::new();
    let response = client
        .get("https://api.modrinth.com/v2/search")
        .query(&[
            ("query", query),
            ("limit", "30".to_string()),
            ("index", sort_index),
            (
                "facets",
                serde_json::to_string(&facets).map_err(|err| err.to_string())?,
            ),
        ])
        .send()
        .await
        .map_err(|err| format!("No se pudo consultar Modrinth: {err}"))?;

    if !response.status().is_success() {
        return Err(format!("Modrinth respondió con {}", response.status()));
    }

    let payload: serde_json::Value = response
        .json()
        .await
        .map_err(|err| format!("Respuesta inválida de Modrinth: {err}"))?;

    let hits = payload
        .get("hits")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    Ok(hits
        .into_iter()
        .map(|hit| {
            let categories = hit
                .get("categories")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|item| item.as_str().map(|s| s.to_string()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let versions = hit
                .get("versions")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|item| item.as_str().map(|s| s.to_string()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            CatalogItem {
                id: hit
                    .get("project_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                source: "Modrinth".to_string(),
                title: hit
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Sin título")
                    .to_string(),
                description: hit
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                image: hit
                    .get("icon_url")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                author: hit
                    .get("author")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string(),
                downloads: hit.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0),
                updated_at: hit
                    .get("date_modified")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                minecraft_versions: versions,
                loaders: categories.clone(),
                project_type: hit
                    .get("project_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string(),
                tags: categories,
            }
        })
        .collect())
}

#[tauri::command]
pub async fn fetch_curseforge_catalog(
    query: String,
    class_id: Option<u32>,
    sort_field: u8,
    mc_version: Option<String>,
    loader: Option<String>,
) -> Result<Vec<CatalogItem>, String> {
    let api_key = std::env::var("CURSEFORGE_API_KEY").unwrap_or_else(|_| {
        "$2a$10$jK7YyZHdUNTDlcME9Egd6.Zt5RananLQKn/tpIhmRDezd2.wHGU9G".to_string()
    });

    let mut request = reqwest::Client::new()
        .get("https://api.curseforge.com/v1/mods/search")
        .query(&[
            ("gameId", "432".to_string()),
            ("pageSize", "30".to_string()),
            ("sortField", sort_field.to_string()),
            ("sortOrder", "desc".to_string()),
        ]);

    if !query.trim().is_empty() {
        request = request.query(&[("searchFilter", query)]);
    }
    if let Some(version) = mc_version.filter(|v| !v.trim().is_empty()) {
        request = request.query(&[("gameVersion", version)]);
    }
    if let Some(class_id) = class_id {
        request = request.query(&[("classId", class_id.to_string())]);
    }
    if let Some(loader) = loader.filter(|v| !v.trim().is_empty()) {
        request = request.query(&[("modLoaderType", map_loader(loader).to_string())]);
    }

    let response = request
        .header("x-api-key", api_key)
        .send()
        .await
        .map_err(|err| format!("No se pudo consultar CurseForge: {err}"))?;

    if !response.status().is_success() {
        return Err(format!("CurseForge respondió con {}", response.status()));
    }

    let payload: serde_json::Value = response
        .json()
        .await
        .map_err(|err| format!("Respuesta inválida de CurseForge: {err}"))?;

    let data = payload
        .get("data")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    Ok(data
        .into_iter()
        .map(|entry| {
            let latest_indexes = entry
                .get("latestFilesIndexes")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            let game_versions = latest_indexes
                .iter()
                .filter_map(|item| {
                    item.get("gameVersion")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                })
                .collect::<Vec<_>>();
            let loaders = latest_indexes
                .iter()
                .filter_map(|item| item.get("modLoader").and_then(|v| v.as_i64()))
                .map(curse_loader_name)
                .collect::<Vec<_>>();

            CatalogItem {
                id: entry
                    .get("id")
                    .and_then(|v| v.as_u64())
                    .unwrap_or_default()
                    .to_string(),
                source: "CurseForge".to_string(),
                title: entry
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Sin título")
                    .to_string(),
                description: entry
                    .get("summary")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                image: entry
                    .get("logo")
                    .and_then(|v| v.get("thumbnailUrl"))
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                author: entry
                    .get("authors")
                    .and_then(|v| v.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|first| first.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string(),
                downloads: entry
                    .get("downloadCount")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                updated_at: entry
                    .get("dateReleased")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                minecraft_versions: game_versions,
                loaders,
                project_type: entry
                    .get("class")
                    .and_then(|v| v.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("-")
                    .to_string(),
                tags: entry
                    .get("categories")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|item| {
                                item.get("name")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string())
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
            }
        })
        .collect())
}

fn map_loader(loader: String) -> u8 {
    match loader.to_ascii_lowercase().as_str() {
        "forge" => 1,
        "fabric" => 4,
        "quilt" => 5,
        "neoforge" => 6,
        _ => 0,
    }
}

fn curse_loader_name(id: i64) -> String {
    match id {
        1 => "forge",
        3 => "liteloader",
        4 => "fabric",
        5 => "quilt",
        6 => "neoforge",
        _ => "unknown",
    }
    .to_string()
}
