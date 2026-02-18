use std::{fs, path::Path};

use sha1::{Digest, Sha1};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    domain::{
        minecraft::{
            argument_resolver::{resolve_launch_arguments, LaunchContext},
            manifest::{ManifestVersionEntry, VersionManifest},
            rule_engine::{evaluate_rules, RuleContext},
        },
        models::instance::InstanceMetadata,
    },
    infrastructure::filesystem::file_ops::write_placeholder_file,
    shared::result::AppResult,
};

const MOJANG_MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const MOJANG_RESOURCES_URL: &str = "https://resources.download.minecraft.net";
const MOJANG_MANIFEST_HOST: &str = "launchermeta.mojang.com";
const MOJANG_PISTON_META_HOST: &str = "piston-meta.mojang.com";
const MOJANG_PISTON_DATA_HOST: &str = "piston-data.mojang.com";
const MOJANG_RESOURCE_HOST: &str = "resources.download.minecraft.net";
const MOJANG_LIBRARIES_HOST: &str = "libraries.minecraft.net";

pub fn build_instance_structure(
    _instance_root: &Path,
    minecraft_root: &Path,
    minecraft_version: &str,
    download_assets_during_creation: bool,
    logs: &mut Vec<String>,
    on_progress: &mut dyn FnMut(u64, u64, String),
) -> AppResult<()> {
    let structure_dirs = [
        minecraft_root.join("assets"),
        minecraft_root.join("libraries"),
        minecraft_root.join("versions").join(minecraft_version),
        minecraft_root.join("mods"),
        minecraft_root.join("config"),
        minecraft_root.join("logs"),
        minecraft_root.join("crash-reports"),
        minecraft_root.join("saves"),
        minecraft_root.join("natives"),
    ];

    for dir in structure_dirs {
        fs::create_dir_all(&dir)
            .map_err(|err| format!("No se pudo crear el directorio {}: {err}", dir.display()))?;
    }
    logs.push("Estructura creada con raíz limpia (minecraft + .instance.json).".to_string());

    let version_file_base = minecraft_root.join("versions").join(minecraft_version);
    let jar_path = version_file_base.join(format!("{minecraft_version}.jar"));
    let json_path = version_file_base.join(format!("{minecraft_version}.json"));

    let version_json = download_version_json(minecraft_version)?;
    let rule_context = RuleContext::current();
    let planned_libraries = planned_library_downloads(&version_json, &rule_context);
    let planned_assets = if download_assets_during_creation {
        planned_asset_downloads(&version_json)?
    } else {
        0
    };
    let total_downloads = 2_u64 + planned_libraries as u64 + planned_assets as u64;
    let mut completed_downloads = 0_u64;

    download_client_jar(&version_json, &jar_path)?;
    completed_downloads += 1;
    on_progress(
        completed_downloads,
        total_downloads,
        format!("Descarga de cliente completada ({completed_downloads}/{total_downloads})."),
    );
    let pretty_version_json =
        serde_json::to_string_pretty(&version_json).map_err(|err| err.to_string())?;
    fs::write(&json_path, pretty_version_json).map_err(|err| {
        format!(
            "No se pudo guardar version.json en {}: {err}",
            json_path.display()
        )
    })?;
    logs.push(format!(
        "version.json oficial guardado en {}.",
        json_path.display()
    ));
    logs.push(format!(
        "client.jar oficial guardado en {}.",
        jar_path.display()
    ));

    let downloaded_libraries = download_libraries(
        &version_json,
        minecraft_root,
        &rule_context,
        &mut completed_downloads,
        total_downloads,
        on_progress,
    )?;
    logs.push(format!(
        "Librerías oficiales descargadas: {} artefactos.",
        downloaded_libraries
    ));

    if download_assets_during_creation {
        let downloaded_assets = download_assets(
            &version_json,
            minecraft_root,
            &mut completed_downloads,
            total_downloads,
            on_progress,
        )?;
        logs.push(format!(
            "Assets oficiales descargados: {} objetos.",
            downloaded_assets
        ));
    } else {
        logs.push(
            "Assets diferidos para acelerar la creación. Se resolverán al ejecutar la instancia."
                .to_string(),
        );
    }

    let launch_context = LaunchContext {
        classpath: "${classpath}".to_string(),
        classpath_separator: if cfg!(target_os = "windows") {
            ";".to_string()
        } else {
            ":".to_string()
        },
        library_directory: minecraft_root.join("libraries").display().to_string(),
        natives_dir: minecraft_root.join("natives").display().to_string(),
        launcher_name: "Interface-2".to_string(),
        launcher_version: env!("CARGO_PKG_VERSION").to_string(),
        auth_player_name: "Player".to_string(),
        auth_uuid: "00000000-0000-0000-0000-000000000000".to_string(),
        auth_access_token: "token-placeholder".to_string(),
        user_type: "msa".to_string(),
        user_properties: "{}".to_string(),
        version_name: minecraft_version.to_string(),
        game_directory: minecraft_root.display().to_string(),
        assets_root: minecraft_root.join("assets").display().to_string(),
        assets_index_name: version_json
            .get("assetIndex")
            .and_then(|v| v.get("id"))
            .and_then(Value::as_str)
            .or(version_json.get("assets").and_then(Value::as_str))
            .unwrap_or(minecraft_version)
            .to_string(),
        version_type: version_json
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("release")
            .to_string(),
        resolution_width: "854".to_string(),
        resolution_height: "480".to_string(),
        clientid: String::new(),
        auth_xuid: String::new(),
        xuid: String::new(),
        quick_play_singleplayer: String::new(),
        quick_play_multiplayer: String::new(),
        quick_play_realms: String::new(),
        quick_play_path: String::new(),
    };

    let resolved =
        resolve_launch_arguments(&version_json, &launch_context, &RuleContext::current())?;
    let launch_profile_path = version_file_base.join("launch_profile.json");
    let launch_profile = json!({
        "mainClass": resolved.main_class,
        "jvm": resolved.jvm,
        "game": resolved.game,
        "all": resolved.all,
        "kind": if version_json.get("arguments").is_some() { "modern" } else { "legacy" }
    });
    fs::write(
        &launch_profile_path,
        serde_json::to_string_pretty(&launch_profile).map_err(|err| err.to_string())?,
    )
    .map_err(|err| {
        format!(
            "No se pudo guardar launch_profile.json en {}: {err}",
            launch_profile_path.display()
        )
    })?;

    logs.push(format!(
        "Argumentos de lanzamiento resueltos ({}): JVM={} | GAME={}",
        if version_json.get("arguments").is_some() {
            "moderno"
        } else {
            "legacy"
        },
        launch_profile["jvm"].as_array().map_or(0, Vec::len),
        launch_profile["game"].as_array().map_or(0, Vec::len)
    ));

    Ok(())
}

fn ensure_official_url(url: &str) -> AppResult<()> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|err| format!("URL oficial inválida: {url}. Error: {err}"))?;
    let host = parsed.host_str().unwrap_or_default();
    let allowed = [
        MOJANG_MANIFEST_HOST,
        MOJANG_PISTON_META_HOST,
        MOJANG_PISTON_DATA_HOST,
        MOJANG_RESOURCE_HOST,
        MOJANG_LIBRARIES_HOST,
    ];
    if !allowed.contains(&host) {
        return Err(format!("URL no oficial bloqueada: {url}"));
    }
    Ok(())
}

fn sha1_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn download_version_json(minecraft_version: &str) -> AppResult<Value> {
    let manifest = reqwest::blocking::get(MOJANG_MANIFEST_URL)
        .map_err(|err| format!("No se pudo descargar version_manifest oficial: {err}"))?
        .error_for_status()
        .map_err(|err| format!("version_manifest respondió con error HTTP: {err}"))?
        .json::<VersionManifest>()
        .map_err(|err| format!("No se pudo deserializar version_manifest oficial: {err}"))?;

    let version = manifest
        .versions
        .into_iter()
        .find(|entry: &ManifestVersionEntry| entry.id == minecraft_version)
        .ok_or_else(|| {
            format!("No se encontró la versión {minecraft_version} en el manifest de Mojang.")
        })?;

    ensure_official_url(&version.url)?;

    reqwest::blocking::get(version.url)
        .map_err(|err| format!("No se pudo descargar version.json oficial: {err}"))?
        .error_for_status()
        .map_err(|err| format!("version.json respondió con error HTTP: {err}"))?
        .json::<Value>()
        .map_err(|err| format!("No se pudo deserializar version.json: {err}"))
}

fn download_client_jar(version_json: &Value, jar_path: &Path) -> AppResult<()> {
    let client_url = version_json
        .get("downloads")
        .and_then(|downloads| downloads.get("client"))
        .and_then(|client| client.get("url"))
        .and_then(Value::as_str)
        .ok_or_else(|| "version.json no incluye downloads.client.url".to_string())?;

    let expected_sha1 = version_json
        .get("downloads")
        .and_then(|downloads| downloads.get("client"))
        .and_then(|client| client.get("sha1"))
        .and_then(Value::as_str)
        .ok_or_else(|| "version.json no incluye downloads.client.sha1".to_string())?;

    download_binary(client_url, jar_path, true, Some(expected_sha1)).map(|_| ())
}

fn download_libraries(
    version_json: &Value,
    minecraft_root: &Path,
    rule_context: &RuleContext,
    completed_downloads: &mut u64,
    total_downloads: u64,
    on_progress: &mut dyn FnMut(u64, u64, String),
) -> AppResult<usize> {
    let mut downloaded = 0usize;

    let libraries = version_json
        .get("libraries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    for library in libraries {
        let rules = library
            .get("rules")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        if !evaluate_rules(&rules, rule_context) {
            continue;
        }

        if let Some((url, path, sha1)) = artifact_download_entry(&library) {
            let output_path = minecraft_root.join("libraries").join(path);
            if download_binary(url, &output_path, false, Some(sha1))? {
                downloaded += 1;
            }
            *completed_downloads += 1;
            on_progress(
                *completed_downloads,
                total_downloads,
                format!("Librerías: {}/{}", *completed_downloads, total_downloads),
            );
        }

        if let Some((url, path, sha1)) = native_download_entry(&library, rule_context) {
            let output_path = minecraft_root.join("libraries").join(path);
            if download_binary(url, &output_path, false, Some(sha1))? {
                downloaded += 1;
            }
            *completed_downloads += 1;
            on_progress(
                *completed_downloads,
                total_downloads,
                format!(
                    "Librerías/Natives: {}/{}",
                    *completed_downloads, total_downloads
                ),
            );
        }
    }

    Ok(downloaded)
}

fn artifact_download_entry(library: &Value) -> Option<(&str, &str, &str)> {
    let artifact = library.get("downloads")?.get("artifact")?;
    Some((
        artifact.get("url")?.as_str()?,
        artifact.get("path")?.as_str()?,
        artifact.get("sha1")?.as_str()?,
    ))
}

fn native_download_entry<'a>(
    library: &'a Value,
    rule_context: &RuleContext,
) -> Option<(&'a str, &'a str, &'a str)> {
    let os_key = match rule_context.os_name {
        crate::domain::minecraft::rule_engine::OsName::Windows => "windows",
        crate::domain::minecraft::rule_engine::OsName::Linux => "linux",
        crate::domain::minecraft::rule_engine::OsName::Macos => "osx",
        crate::domain::minecraft::rule_engine::OsName::Unknown => return None,
    };

    let classifier = library
        .get("natives")?
        .get(os_key)?
        .as_str()?
        .replace("${arch}", &rule_context.arch);

    let entry = library
        .get("downloads")?
        .get("classifiers")?
        .get(classifier)?;

    Some((
        entry.get("url")?.as_str()?,
        entry.get("path")?.as_str()?,
        entry.get("sha1")?.as_str()?,
    ))
}

fn download_assets(
    version_json: &Value,
    minecraft_root: &Path,
    completed_downloads: &mut u64,
    total_downloads: u64,
    on_progress: &mut dyn FnMut(u64, u64, String),
) -> AppResult<usize> {
    let asset_index = version_json
        .get("assetIndex")
        .ok_or_else(|| "version.json no incluye assetIndex".to_string())?;

    let index_url = asset_index
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| "assetIndex.url no está presente".to_string())?;
    let index_id = asset_index
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "assetIndex.id no está presente".to_string())?;

    let indexes_dir = minecraft_root.join("assets").join("indexes");
    fs::create_dir_all(&indexes_dir).map_err(|err| {
        format!(
            "No se pudo crear carpeta de asset indexes {}: {err}",
            indexes_dir.display()
        )
    })?;

    let index_path = indexes_dir.join(format!("{index_id}.json"));
    download_binary(index_url, &index_path, true, None)?;
    *completed_downloads += 1;
    on_progress(
        *completed_downloads,
        total_downloads,
        format!("Asset index: {}/{}", *completed_downloads, total_downloads),
    );

    let raw_index = fs::read_to_string(&index_path).map_err(|err| {
        format!(
            "No se pudo leer asset index {}: {err}",
            index_path.display()
        )
    })?;
    let parsed_index = serde_json::from_str::<AssetIndex>(&raw_index)
        .map_err(|err| format!("asset index inválido {}: {err}", index_path.display()))?;

    let mut downloaded = 0usize;
    for object in parsed_index.objects.values() {
        if object.hash.len() < 2 {
            continue;
        }
        let prefix = &object.hash[..2];
        let object_path = minecraft_root
            .join("assets")
            .join("objects")
            .join(prefix)
            .join(&object.hash);
        let url = format!("{MOJANG_RESOURCES_URL}/{prefix}/{}", object.hash);
        if download_binary(&url, &object_path, false, Some(&object.hash))? {
            downloaded += 1;
        }
        *completed_downloads += 1;
        if *completed_downloads % 25 == 0 || *completed_downloads == total_downloads {
            on_progress(
                *completed_downloads,
                total_downloads,
                format!("Assets: {}/{}", *completed_downloads, total_downloads),
            );
        }
    }

    Ok(downloaded)
}

fn planned_library_downloads(version_json: &Value, rule_context: &RuleContext) -> usize {
    version_json
        .get("libraries")
        .and_then(Value::as_array)
        .map(|libraries| {
            libraries
                .iter()
                .filter(|library| {
                    let rules = library
                        .get("rules")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    evaluate_rules(&rules, rule_context)
                })
                .map(|library| {
                    let mut count = 0;
                    if artifact_download_entry(library).is_some() {
                        count += 1;
                    }
                    if native_download_entry(library, rule_context).is_some() {
                        count += 1;
                    }
                    count
                })
                .sum()
        })
        .unwrap_or(0)
}

fn planned_asset_downloads(version_json: &Value) -> AppResult<usize> {
    let asset_index = version_json
        .get("assetIndex")
        .ok_or_else(|| "version.json no incluye assetIndex".to_string())?;
    let index_url = asset_index
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| "assetIndex.url no está presente".to_string())?;

    let parsed_index = reqwest::blocking::get(index_url)
        .map_err(|err| format!("No se pudo descargar asset index oficial: {err}"))?
        .error_for_status()
        .map_err(|err| format!("asset index devolvió error HTTP: {err}"))?
        .json::<AssetIndex>()
        .map_err(|err| format!("No se pudo deserializar asset index oficial: {err}"))?;

    Ok(parsed_index.objects.len())
}

fn download_binary(
    url: &str,
    target_path: &Path,
    force: bool,
    expected_sha1: Option<&str>,
) -> AppResult<bool> {
    if !force && target_path.exists() {
        return Ok(false);
    }

    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "No se pudo crear directorio {} para descarga: {err}",
                parent.display()
            )
        })?;
    }

    ensure_official_url(url)?;

    let bytes = reqwest::blocking::get(url)
        .map_err(|err| format!("No se pudo descargar recurso oficial {url}: {err}"))?
        .error_for_status()
        .map_err(|err| format!("Recurso oficial devolvió error HTTP ({url}): {err}"))?
        .bytes()
        .map_err(|err| format!("No se pudo leer bytes descargados de {url}: {err}"))?;

    if let Some(expected) = expected_sha1 {
        let actual = sha1_hex(bytes.as_ref());
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(format!(
                "SHA1 inválido para {}. Esperado: {expected}, actual: {actual}",
                url
            ));
        }
    }

    write_placeholder_file(target_path, "")?;
    fs::write(target_path, &bytes).map_err(|err| {
        format!(
            "No se pudo guardar archivo descargado en {}: {err}",
            target_path.display()
        )
    })?;

    Ok(true)
}

#[derive(Debug, Deserialize)]
struct AssetIndex {
    objects: std::collections::HashMap<String, AssetObject>,
}

#[derive(Debug, Deserialize)]
struct AssetObject {
    hash: String,
}

pub fn persist_instance_metadata(
    instance_root: &Path,
    metadata: &InstanceMetadata,
    logs: &mut Vec<String>,
) -> AppResult<()> {
    let metadata_path = instance_root.join(".instance.json");
    let metadata_content = serde_json::to_string_pretty(metadata).map_err(|err| err.to_string())?;
    fs::write(&metadata_path, metadata_content).map_err(|err| {
        format!(
            "No se pudo guardar la metadata de la instancia en {}: {err}",
            metadata_path.display()
        )
    })?;
    logs.push(format!(
        "Metadata guardada en {} (java: {}).",
        metadata_path.display(),
        metadata.java_path
    ));
    Ok(())
}
