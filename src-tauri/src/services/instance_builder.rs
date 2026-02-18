use std::{fs, path::Path};

use serde_json::{json, Value};

use crate::{
    domain::{
        minecraft::{
            argument_resolver::{resolve_launch_arguments, LaunchContext},
            manifest::{ManifestVersionEntry, VersionManifest},
            rule_engine::RuleContext,
        },
        models::instance::InstanceMetadata,
    },
    infrastructure::filesystem::file_ops::write_placeholder_file,
    shared::result::AppResult,
};

const MOJANG_MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

pub fn build_instance_structure(
    instance_root: &Path,
    minecraft_root: &Path,
    minecraft_version: &str,
    logs: &mut Vec<String>,
) -> AppResult<()> {
    let structure_dirs = [
        instance_root.join("logs"),
        instance_root.join("mods"),
        instance_root.join("config"),
        instance_root.join("resourcepacks"),
        instance_root.join("shaderpacks"),
        instance_root.join("saves"),
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
    logs.push("Estructura interna de instancia y .minecraft creada.".to_string());

    let version_file_base = minecraft_root.join("versions").join(minecraft_version);
    let jar_path = version_file_base.join(format!("{minecraft_version}.jar"));
    let json_path = version_file_base.join(format!("{minecraft_version}.json"));

    write_placeholder_file(&jar_path, "placeholder minecraft jar")?;

    let version_json = download_version_json(minecraft_version)?;
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

    let launch_context = LaunchContext {
        classpath: "${classpath}".to_string(),
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

fn download_version_json(minecraft_version: &str) -> AppResult<Value> {
    let manifest = reqwest::blocking::get(MOJANG_MANIFEST_URL)
        .map_err(|err| format!("No se pudo descargar version_manifest_v2: {err}"))?
        .error_for_status()
        .map_err(|err| format!("version_manifest_v2 respondió con error HTTP: {err}"))?
        .json::<VersionManifest>()
        .map_err(|err| format!("No se pudo deserializar version_manifest_v2: {err}"))?;

    let version = manifest
        .versions
        .into_iter()
        .find(|entry: &ManifestVersionEntry| entry.id == minecraft_version)
        .ok_or_else(|| {
            format!("No se encontró la versión {minecraft_version} en el manifest de Mojang.")
        })?;

    reqwest::blocking::get(version.url)
        .map_err(|err| format!("No se pudo descargar version.json oficial: {err}"))?
        .error_for_status()
        .map_err(|err| format!("version.json respondió con error HTTP: {err}"))?
        .json::<Value>()
        .map_err(|err| format!("No se pudo deserializar version.json: {err}"))
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
