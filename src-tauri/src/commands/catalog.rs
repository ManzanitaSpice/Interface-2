use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogSearchRequest {
    pub search: String,
    pub category: Option<String>,
    pub curseforge_class_id: Option<u32>,
    pub platform: String,
    pub mc_version: Option<String>,
    pub loader: Option<String>,
    pub modrinth_sort: String,
    pub curseforge_sort_field: u32,
    pub limit: Option<u32>,
}

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
    pub size: String,
    pub minecraft_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub project_type: String,
    pub tags: Vec<String>,
}

#[tauri::command]
pub fn search_catalogs(request: CatalogSearchRequest) -> Result<Vec<CatalogItem>, String> {
    let client = Client::builder()
        .user_agent("Interface-2/0.1")
        .build()
        .map_err(|err| format!("No se pudo inicializar cliente HTTP: {err}"))?;

    let limit = request.limit.unwrap_or(30).clamp(1, 50);
    let mut output = Vec::new();

    if request.platform != "Curseforge" {
        output.extend(fetch_modrinth(&client, &request, limit)?);
    }
    if request.platform != "Modrinth" {
        output.extend(fetch_curseforge(&client, &request, limit)?);
    }

    Ok(output)
}

fn fetch_modrinth(
    client: &Client,
    request: &CatalogSearchRequest,
    limit: u32,
) -> Result<Vec<CatalogItem>, String> {
    let mut facets: Vec<Vec<String>> = Vec::new();
    if let Some(project_type) = request.category.as_ref().filter(|v| !v.is_empty()) {
        facets.push(vec![format!("project_type:{project_type}")]);
    }
    if let Some(version) = request.mc_version.as_ref().filter(|v| !v.is_empty()) {
        facets.push(vec![format!("versions:{version}")]);
    }
    if let Some(loader) = request.loader.as_ref().filter(|v| !v.is_empty()) {
        facets.push(vec![format!("categories:{}", loader.to_ascii_lowercase())]);
    }

    let mut params = vec![
        ("query", request.search.clone()),
        ("limit", limit.to_string()),
        ("index", request.modrinth_sort.clone()),
    ];
    if !facets.is_empty() {
        let raw = serde_json::to_string(&facets)
            .map_err(|err| format!("No se pudo serializar filtros de Modrinth: {err}"))?;
        params.push(("facets", raw));
    }

    let response = client
        .get("https://api.modrinth.com/v2/search")
        .query(&params)
        .send()
        .map_err(|err| format!("Error consultando Modrinth: {err}"))?;

    if !response.status().is_success() {
        return Err(format!("Modrinth respondió con {}", response.status()));
    }

    let payload: Value = response
        .json()
        .map_err(|err| format!("Respuesta inválida de Modrinth: {err}"))?;

    let hits = payload
        .get("hits")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    Ok(hits
        .iter()
        .map(|hit| {
            let categories = hit
                .get("categories")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let versions = hit
                .get("versions")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            CatalogItem {
                id: hit
                    .get("project_id")
                    .and_then(Value::as_str)
                    .unwrap_or("-")
                    .to_string(),
                source: "Modrinth".to_string(),
                title: hit
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("Sin título")
                    .to_string(),
                description: hit
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                image: hit
                    .get("icon_url")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                author: hit
                    .get("author")
                    .and_then(Value::as_str)
                    .unwrap_or("-")
                    .to_string(),
                downloads: hit.get("downloads").and_then(Value::as_u64).unwrap_or(0),
                updated_at: hit
                    .get("date_modified")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                size: "-".to_string(),
                minecraft_versions: versions,
                loaders: categories.clone(),
                project_type: hit
                    .get("project_type")
                    .and_then(Value::as_str)
                    .unwrap_or("-")
                    .to_string(),
                tags: categories,
            }
        })
        .collect())
}

fn fetch_curseforge(
    client: &Client,
    request: &CatalogSearchRequest,
    limit: u32,
) -> Result<Vec<CatalogItem>, String> {
    let api_key = std::env::var("CURSEFORGE_API_KEY").unwrap_or_else(|_| {
        "$2a$10$jK7YyZHdUNTDlcME9Egd6.Zt5RananLQKn/tpIhmRDezd2.wHGU9G".to_string()
    });

    let mut params = vec![
        ("gameId", "432".to_string()),
        ("pageSize", limit.to_string()),
        ("sortField", request.curseforge_sort_field.to_string()),
        ("sortOrder", "desc".to_string()),
    ];

    if !request.search.trim().is_empty() {
        params.push(("searchFilter", request.search.clone()));
    }
    if let Some(version) = request.mc_version.as_ref().filter(|v| !v.is_empty()) {
        params.push(("gameVersion", version.clone()));
    }
    if let Some(class_id) = request.curseforge_class_id {
        params.push(("classId", class_id.to_string()));
    }

    let response = client
        .get("https://api.curseforge.com/v1/mods/search")
        .header("x-api-key", api_key)
        .query(&params)
        .send()
        .map_err(|err| format!("Error consultando CurseForge: {err}"))?;

    if !response.status().is_success() {
        return Err(format!("CurseForge respondió con {}", response.status()));
    }

    let payload: Value = response
        .json()
        .map_err(|err| format!("Respuesta inválida de CurseForge: {err}"))?;

    let data = payload
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    Ok(data
        .iter()
        .map(|entry| {
            let latest_indexes = entry
                .get("latestFilesIndexes")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let game_versions = latest_indexes
                .iter()
                .filter_map(|item| item.get("gameVersion").and_then(Value::as_str))
                .map(str::to_string)
                .collect::<Vec<_>>();
            let loaders = latest_indexes
                .iter()
                .filter_map(|item| item.get("modLoader").and_then(Value::as_str))
                .map(str::to_string)
                .collect::<Vec<_>>();
            let tags = entry
                .get("categories")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(|item| item.get("name").and_then(Value::as_str))
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            CatalogItem {
                id: entry
                    .get("id")
                    .map(|value| value.to_string().replace('"', ""))
                    .unwrap_or_else(|| "-".to_string()),
                source: "CurseForge".to_string(),
                title: entry
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("Sin título")
                    .to_string(),
                description: entry
                    .get("summary")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                image: entry
                    .get("logo")
                    .and_then(|logo| logo.get("thumbnailUrl"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                author: entry
                    .get("authors")
                    .and_then(Value::as_array)
                    .and_then(|authors| authors.first())
                    .and_then(|author| author.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("-")
                    .to_string(),
                downloads: entry
                    .get("downloadCount")
                    .and_then(Value::as_f64)
                    .map(|v| v as u64)
                    .unwrap_or(0),
                updated_at: entry
                    .get("dateReleased")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                size: "-".to_string(),
                minecraft_versions: game_versions,
                loaders,
                project_type: entry
                    .get("class")
                    .and_then(|class| class.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("-")
                    .to_string(),
                tags,
            }
        })
        .collect())
}
