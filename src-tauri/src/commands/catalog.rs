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
    pub page: Option<u32>,
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogSearchResponse {
    pub items: Vec<CatalogItem>,
    pub page: u32,
    pub limit: u32,
    pub has_more: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogDetailRequest {
    pub id: String,
    pub source: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogVersion {
    pub id: String,
    pub version_type: String,
    pub name: String,
    pub published_at: String,
    pub mod_loader: String,
    pub game_version: String,
    pub download_url: String,
    pub file_url: String,
    pub required_dependencies: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogExternalLink {
    pub label: String,
    pub url: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogDetailResponse {
    pub id: String,
    pub source: String,
    pub title: String,
    pub author: String,
    pub description: String,
    pub body_html: String,
    pub changelog_html: String,
    pub url: String,
    pub image: String,
    pub links: Vec<CatalogExternalLink>,
    pub gallery: Vec<String>,
    pub versions: Vec<CatalogVersion>,
    pub comments_url: String,
}

#[tauri::command]
pub fn search_catalogs(request: CatalogSearchRequest) -> Result<CatalogSearchResponse, String> {
    let client = Client::builder()
        .user_agent("Interface-2/0.1")
        .timeout(std::time::Duration::from_secs(12))
        .build()
        .map_err(|err| format!("No se pudo inicializar cliente HTTP: {err}"))?;

    let limit = request.limit.unwrap_or(30).clamp(1, 50);
    let page = request.page.unwrap_or(1).max(1);
    let search = request.search.trim().chars().take(80).collect::<String>();
    let offset = (page - 1) * limit;
    let mut output = Vec::new();
    let mut has_more = false;

    let platform = request.platform.trim().to_ascii_lowercase();

    let per_source_limit = if platform == "all" || platform == "todas" {
        (limit / 2).max(1)
    } else {
        limit
    };

    let mut normalized_request = request;
    normalized_request.search = search;

    if platform != "curseforge" {
        let (items, source_has_more) =
            fetch_modrinth(&client, &normalized_request, per_source_limit, offset)?;
        has_more = has_more || source_has_more;
        output.extend(items);
    }
    if platform != "modrinth" {
        let (items, source_has_more) =
            fetch_curseforge(&client, &normalized_request, per_source_limit, offset)?;
        has_more = has_more || source_has_more;
        output.extend(items);
    }

    let search_tokens = tokenize_search(&normalized_request.search);
    output.sort_by(|a, b| compare_catalog_items(a, b, &search_tokens));

    if output.len() > limit as usize {
        output.truncate(limit as usize);
    }

    Ok(CatalogSearchResponse {
        items: output,
        page,
        limit,
        has_more,
    })
}

#[tauri::command]
pub fn get_catalog_detail(request: CatalogDetailRequest) -> Result<CatalogDetailResponse, String> {
    let client = Client::builder()
        .user_agent("Interface-2/0.1")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|err| format!("No se pudo inicializar cliente HTTP: {err}"))?;

    let source = request.source.trim().to_ascii_lowercase();
    if source == "modrinth" {
        return fetch_modrinth_detail(&client, &request.id);
    }
    if source == "curseforge" {
        return fetch_curseforge_detail(&client, &request.id);
    }

    Err(format!(
        "Fuente de catálogo no soportada: {}",
        request.source
    ))
}

fn fetch_modrinth_detail(client: &Client, id: &str) -> Result<CatalogDetailResponse, String> {
    let project: Value = client
        .get(format!("https://api.modrinth.com/v2/project/{id}"))
        .send()
        .map_err(|err| format!("Error consultando detalle de Modrinth: {err}"))?
        .json()
        .map_err(|err| format!("Respuesta inválida de Modrinth (project): {err}"))?;

    let versions_payload: Value = client
        .get(format!("https://api.modrinth.com/v2/project/{id}/version"))
        .query(&[("limit", "80")])
        .send()
        .map_err(|err| format!("Error consultando versiones de Modrinth: {err}"))?
        .json()
        .map_err(|err| format!("Respuesta inválida de Modrinth (versions): {err}"))?;

    let versions = versions_payload
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|entry| {
            let loaders = entry
                .get("loaders")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_else(|| "-".to_string());
            let game_versions = entry
                .get("game_versions")
                .and_then(Value::as_array)
                .and_then(|list| list.first())
                .and_then(Value::as_str)
                .unwrap_or("-")
                .to_string();
            let download_url = entry
                .get("files")
                .and_then(Value::as_array)
                .and_then(|files| files.first())
                .and_then(|file| file.get("url"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();

            CatalogVersion {
                id: entry
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                version_type: entry
                    .get("version_type")
                    .and_then(Value::as_str)
                    .unwrap_or("release")
                    .to_string(),
                name: entry
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("-")
                    .to_string(),
                published_at: entry
                    .get("date_published")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                mod_loader: loaders,
                game_version: game_versions,
                download_url: download_url.clone(),
                file_url: entry
                    .get("id")
                    .and_then(Value::as_str)
                    .map(|version_id| {
                        format!("https://modrinth.com/project/{id}/version/{version_id}")
                    })
                    .unwrap_or(download_url),
                required_dependencies: entry
                    .get("dependencies")
                    .and_then(Value::as_array)
                    .map(|dependencies| {
                        dependencies
                            .iter()
                            .filter(|dependency| {
                                dependency
                                    .get("dependency_type")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .eq_ignore_ascii_case("required")
                            })
                            .filter_map(|dependency| {
                                dependency
                                    .get("project_id")
                                    .and_then(Value::as_str)
                                    .map(str::to_string)
                                    .or_else(|| {
                                        dependency
                                            .get("version_id")
                                            .and_then(Value::as_str)
                                            .map(str::to_string)
                                    })
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
            }
        })
        .collect::<Vec<_>>();

    let gallery = project
        .get("gallery")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("url").and_then(Value::as_str))
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut links = vec![CatalogExternalLink {
        label: "Página".to_string(),
        url: format!("https://modrinth.com/project/{id}"),
    }];
    if let Some(wiki) = project.get("wiki_url").and_then(Value::as_str) {
        if !wiki.is_empty() {
            links.push(CatalogExternalLink {
                label: "Wiki".to_string(),
                url: wiki.to_string(),
            });
        }
    }
    if let Some(discord) = project.get("discord_url").and_then(Value::as_str) {
        if !discord.is_empty() {
            links.push(CatalogExternalLink {
                label: "Discord".to_string(),
                url: discord.to_string(),
            });
        }
    }

    Ok(CatalogDetailResponse {
        id: id.to_string(),
        source: "Modrinth".to_string(),
        title: project
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("Sin título")
            .to_string(),
        author: project
            .get("team")
            .and_then(Value::as_str)
            .unwrap_or("-")
            .to_string(),
        description: project
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        body_html: project
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .replace('\n', "<br />"),
        changelog_html: "Consulta la pestaña Versions para revisar notas por versión en Modrinth."
            .to_string(),
        url: format!("https://modrinth.com/project/{id}"),
        image: project
            .get("icon_url")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        links,
        gallery,
        versions,
        comments_url: format!("https://modrinth.com/project/{id}"),
    })
}

fn fetch_curseforge_detail(client: &Client, id: &str) -> Result<CatalogDetailResponse, String> {
    let api_key = std::env::var("CURSEFORGE_API_KEY").unwrap_or_else(|_| {
        "$2a$10$jK7YyZHdUNTDlcME9Egd6.Zt5RananLQKn/tpIhmRDezd2.wHGU9G".to_string()
    });

    let project: Value = client
        .get(format!("https://api.curseforge.com/v1/mods/{id}"))
        .header("x-api-key", &api_key)
        .send()
        .map_err(|err| format!("Error consultando detalle de CurseForge: {err}"))?
        .json()
        .map_err(|err| format!("Respuesta inválida de CurseForge (mod): {err}"))?;

    let description_payload: Value = client
        .get(format!(
            "https://api.curseforge.com/v1/mods/{id}/description"
        ))
        .header("x-api-key", &api_key)
        .send()
        .map_err(|err| format!("Error consultando descripción de CurseForge: {err}"))?
        .json()
        .map_err(|err| format!("Respuesta inválida de CurseForge (description): {err}"))?;

    let files_payload: Value = client
        .get(format!("https://api.curseforge.com/v1/mods/{id}/files"))
        .header("x-api-key", &api_key)
        .query(&[("pageSize", "80")])
        .send()
        .map_err(|err| format!("Error consultando versiones de CurseForge: {err}"))?
        .json()
        .map_err(|err| format!("Respuesta inválida de CurseForge (files): {err}"))?;

    let project_data = project.get("data").cloned().unwrap_or(Value::Null);

    let versions = files_payload
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|entry| {
            let versions = entry
                .get("sortableGameVersions")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let game_version = versions
                .iter()
                .find(|item| item.get("gameVersionName").is_some())
                .and_then(|item| item.get("gameVersionName"))
                .and_then(Value::as_str)
                .unwrap_or("-")
                .to_string();
            let mod_loader = versions
                .iter()
                .find(|item| item.get("gameVersionPadded").is_some())
                .and_then(|_| entry.get("releaseType"))
                .and_then(Value::as_u64)
                .map(|release_type| match release_type {
                    1 => "Release",
                    2 => "Beta",
                    3 => "Alpha",
                    _ => "-",
                })
                .unwrap_or("-")
                .to_string();

            CatalogVersion {
                id: entry
                    .get("id")
                    .and_then(Value::as_u64)
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                version_type: entry
                    .get("displayName")
                    .and_then(Value::as_str)
                    .map(|name| {
                        if name.to_ascii_lowercase().contains("alpha") {
                            "Alpha".to_string()
                        } else if name.to_ascii_lowercase().contains("beta") {
                            "Beta".to_string()
                        } else {
                            "Release".to_string()
                        }
                    })
                    .unwrap_or_else(|| "Release".to_string()),
                name: entry
                    .get("fileName")
                    .and_then(Value::as_str)
                    .unwrap_or("-")
                    .to_string(),
                published_at: entry
                    .get("fileDate")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                mod_loader,
                game_version,
                download_url: entry
                    .get("downloadUrl")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                file_url: entry
                    .get("id")
                    .and_then(Value::as_u64)
                    .map(|file_id| {
                        format!("https://www.curseforge.com/minecraft/mc-mods/{id}/files/{file_id}")
                    })
                    .unwrap_or_default(),
                required_dependencies: entry
                    .get("dependencies")
                    .and_then(Value::as_array)
                    .map(|dependencies| {
                        dependencies
                            .iter()
                            .filter(|dependency| {
                                dependency.get("relationType").and_then(Value::as_u64) == Some(3)
                            })
                            .filter_map(|dependency| {
                                dependency
                                    .get("modId")
                                    .and_then(Value::as_u64)
                                    .map(|value| value.to_string())
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
            }
        })
        .collect::<Vec<_>>();

    let gallery = project_data
        .get("screenshots")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("url").and_then(Value::as_str))
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut links = vec![CatalogExternalLink {
        label: "Página".to_string(),
        url: format!(
            "https://www.curseforge.com/minecraft/mc-mods/{}",
            project_data
                .get("slug")
                .and_then(Value::as_str)
                .unwrap_or(id)
        ),
    }];
    if let Some(website) = project_data
        .get("links")
        .and_then(|v| v.get("websiteUrl"))
        .and_then(Value::as_str)
    {
        if !website.is_empty() {
            links.push(CatalogExternalLink {
                label: "Sitio".to_string(),
                url: website.to_string(),
            });
        }
    }
    if let Some(wiki) = project_data
        .get("links")
        .and_then(|v| v.get("wikiUrl"))
        .and_then(Value::as_str)
    {
        if !wiki.is_empty() {
            links.push(CatalogExternalLink {
                label: "Wiki".to_string(),
                url: wiki.to_string(),
            });
        }
    }
    if let Some(discord) = project_data
        .get("links")
        .and_then(|v| v.get("issuesUrl"))
        .and_then(Value::as_str)
    {
        if !discord.is_empty() {
            links.push(CatalogExternalLink {
                label: "Issues".to_string(),
                url: discord.to_string(),
            });
        }
    }

    Ok(CatalogDetailResponse {
        id: id.to_string(),
        source: "CurseForge".to_string(),
        title: project_data
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Sin título")
            .to_string(),
        author: project_data
            .get("authors")
            .and_then(Value::as_array)
            .and_then(|authors| authors.first())
            .and_then(|author| author.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("-")
            .to_string(),
        description: project_data
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        body_html: description_payload
            .get("data")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        changelog_html: "Consulta la tabla de versiones para revisar los cambios del proyecto."
            .to_string(),
        url: project_data
            .get("links")
            .and_then(|links| links.get("websiteUrl"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        image: project_data
            .get("logo")
            .and_then(|logo| logo.get("thumbnailUrl"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        links,
        gallery,
        versions,
        comments_url: project_data
            .get("links")
            .and_then(|links| links.get("issuesUrl"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

fn fetch_modrinth(
    client: &Client,
    request: &CatalogSearchRequest,
    limit: u32,
    offset: u32,
) -> Result<(Vec<CatalogItem>, bool), String> {
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
        ("offset", offset.to_string()),
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

    let total_hits = payload
        .get("total_hits")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let has_more = (offset as u64 + hits.len() as u64) < total_hits;

    Ok((
        hits.iter()
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
            .collect(),
        has_more,
    ))
}

fn fetch_curseforge(
    client: &Client,
    request: &CatalogSearchRequest,
    limit: u32,
    offset: u32,
) -> Result<(Vec<CatalogItem>, bool), String> {
    let api_key = std::env::var("CURSEFORGE_API_KEY").unwrap_or_else(|_| {
        "$2a$10$jK7YyZHdUNTDlcME9Egd6.Zt5RananLQKn/tpIhmRDezd2.wHGU9G".to_string()
    });

    let mut params = vec![
        ("gameId", "432".to_string()),
        ("pageSize", limit.to_string()),
        ("index", offset.to_string()),
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
    if let Some(loader) = request.loader.as_ref().filter(|v| !v.is_empty()) {
        if let Some(mod_loader_type) = map_curseforge_loader_type(loader) {
            params.push(("modLoaderType", mod_loader_type.to_string()));
        }
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

    let total_count = payload
        .get("pagination")
        .and_then(|value| value.get("totalCount"))
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let has_more = (offset as u64 + data.len() as u64) < total_count;

    Ok((
        data.iter()
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
            .collect(),
        has_more,
    ))
}

fn map_curseforge_loader_type(loader: &str) -> Option<u32> {
    match loader.trim().to_ascii_lowercase().as_str() {
        "forge" => Some(1),
        "fabric" => Some(4),
        "quilt" => Some(5),
        "neoforge" => Some(6),
        _ => None,
    }
}

fn tokenize_search(search: &str) -> Vec<String> {
    let mut tokens = search
        .split_whitespace()
        .map(|token| token.trim().to_ascii_lowercase())
        .filter(|token| token.len() >= 2)
        .collect::<Vec<_>>();
    tokens.sort();
    tokens.dedup();
    tokens
}

fn score_catalog_item(item: &CatalogItem, search_tokens: &[String]) -> i64 {
    let mut score = 0i64;

    if search_tokens.is_empty() {
        score += 10;
    }

    let title = item.title.to_ascii_lowercase();
    let description = item.description.to_ascii_lowercase();
    let author = item.author.to_ascii_lowercase();
    let project_type = item.project_type.to_ascii_lowercase();
    let tags = item
        .tags
        .iter()
        .map(|tag| tag.to_ascii_lowercase())
        .collect::<Vec<_>>();

    for token in search_tokens {
        if title == *token {
            score += 80;
        } else if title.starts_with(token) {
            score += 42;
        } else if title.contains(token) {
            score += 24;
        }

        if description.contains(token) {
            score += 8;
        }

        if author.contains(token) {
            score += 8;
        }

        if tags.iter().any(|tag| tag.contains(token)) {
            score += 14;
        }

        if project_type.contains(token) {
            score += 10;
        }

        if item
            .minecraft_versions
            .iter()
            .any(|version| version.eq_ignore_ascii_case(token))
        {
            score += 12;
        }

        if item
            .loaders
            .iter()
            .any(|loader| loader.to_ascii_lowercase().contains(token))
        {
            score += 10;
        }
    }

    let download_bonus = (item.downloads as f64).log10().max(0.0) as i64;
    let source_bonus = if item.source == "Modrinth" { 1 } else { 0 };
    let exact_phrase_bonus =
        if !search_tokens.is_empty() && title.contains(&search_tokens.join(" ")) {
            28
        } else {
            0
        };

    score + download_bonus + source_bonus + exact_phrase_bonus
}

fn compare_catalog_items(
    a: &CatalogItem,
    b: &CatalogItem,
    search_tokens: &[String],
) -> std::cmp::Ordering {
    let score_a = score_catalog_item(a, search_tokens);
    let score_b = score_catalog_item(b, search_tokens);

    score_b
        .cmp(&score_a)
        .then_with(|| b.downloads.cmp(&a.downloads))
        .then_with(|| b.updated_at.cmp(&a.updated_at))
        .then_with(|| a.title.cmp(&b.title))
}
