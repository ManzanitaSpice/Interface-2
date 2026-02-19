use std::{fs, path::Path};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    domain::{
        minecraft::manifest::{ManifestVersionEntry, VersionManifest},
        models::instance::InstanceMetadata,
    },
    infrastructure::{
        downloader::queue::{build_official_client, download_with_retry},
        filesystem::file_ops::write_placeholder_file,
    },
    services::loader_installer::install_loader_if_needed,
    shared::result::AppResult,
};

const MOJANG_MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const MOJANG_MANIFEST_HOST: &str = "launchermeta.mojang.com";
const MOJANG_PISTON_META_HOST: &str = "piston-meta.mojang.com";
const MOJANG_PISTON_DATA_HOST: &str = "piston-data.mojang.com";
const MOJANG_LIBRARIES_HOST: &str = "libraries.minecraft.net";

pub fn build_instance_structure(
    instance_root: &Path,
    minecraft_root: &Path,
    minecraft_version: &str,
    loader: &str,
    loader_version: &str,
    java_exec: &Path,
    logs: &mut Vec<String>,
    on_progress: &mut dyn FnMut(u64, u64, String),
) -> AppResult<String> {
    let launcher_root = instance_root
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| {
            format!(
                "No se pudo resolver launcher root desde {}",
                instance_root.display()
            )
        })?;

    // FASE 2
    for dir in [
        minecraft_root.to_path_buf(),
        instance_root.join("natives"),
        instance_root.join("logs"),
    ] {
        fs::create_dir_all(&dir)
            .map_err(|err| format!("No se pudo crear el directorio {}: {err}", dir.display()))?;
    }
    logs.push("FASE 2: Estructura base creada (minecraft/, natives/, logs/).".to_string());

    // FASE 3
    on_progress(1, 4, "FASE 3: Resolviendo versión base...".to_string());
    let version_json =
        ensure_global_version_cache(launcher_root, instance_root, minecraft_version)?;
    logs.push(format!(
        "FASE 3: version_manifest y version.json cacheados globalmente para {}.",
        minecraft_version
    ));

    // FASE 4
    on_progress(2, 4, "FASE 4: Preparando loader...".to_string());
    let effective_version_id = prepare_loader(
        minecraft_root,
        minecraft_version,
        loader,
        loader_version,
        java_exec,
        logs,
    )?;

    // FASE 5 (opcional pro): solo pre-verificar
    on_progress(3, 4, "FASE 5: Pre-verificación opcional...".to_string());
    let lib_count = version_json
        .get("libraries")
        .and_then(Value::as_array)
        .map_or(0, |libs| libs.len());
    let has_asset_index = version_json.get("assetIndex").is_some();
    logs.push(format!(
        "FASE 5: Pre-verificación => libraries declaradas: {lib_count}, assetIndex presente: {has_asset_index}. No se descargaron assets completos."
    ));

    // FASE 6
    on_progress(4, 4, "FASE 6: Finalizando instancia (READY)...".to_string());
    logs.push("FASE 6: Instancia marcada para finalizar en estado READY.".to_string());

    Ok(effective_version_id)
}

fn prepare_loader(
    minecraft_root: &Path,
    minecraft_version: &str,
    loader: &str,
    loader_version: &str,
    java_exec: &Path,
    logs: &mut Vec<String>,
) -> AppResult<String> {
    let normalized_loader = loader.trim().to_ascii_lowercase();
    if normalized_loader.is_empty() || normalized_loader == "vanilla" {
        logs.push("FASE 4: VANILLA => no se requiere trabajo extra.".to_string());
        return Ok(minecraft_version.to_string());
    }

    if normalized_loader == "fabric" || normalized_loader == "quilt" {
        let effective = install_loader_if_needed(
            minecraft_root,
            minecraft_version,
            loader,
            loader_version,
            java_exec,
            logs,
        )?;
        logs.push(format!(
            "FASE 4: {loader} preparado en modo lazy (metadata/version profile listo, sin instalación pesada en launch)."
        ));
        return Ok(effective);
    }

    if normalized_loader == "forge" || normalized_loader == "neoforge" {
        let effective = install_loader_if_needed(
            minecraft_root,
            minecraft_version,
            loader,
            loader_version,
            java_exec,
            logs,
        )?;
        logs.push(format!(
            "FASE 4: {loader} preparado. La instalación pesada se ejecutó aquí y NO durante launch."
        ));
        return Ok(effective);
    }

    Err(format!("Loader no soportado todavía: {loader}"))
}

fn ensure_global_version_cache(
    launcher_root: &Path,
    instance_root: &Path,
    minecraft_version: &str,
) -> AppResult<Value> {
    let mc_cache = launcher_root.join("cache").join("minecraft");
    let versions_cache = mc_cache.join("versions");
    fs::create_dir_all(&versions_cache)
        .map_err(|err| format!("No se pudo crear cache global de versiones: {err}"))?;

    let manifest_path = mc_cache.join("version_manifest_v2.json");
    let manifest = if manifest_path.exists() {
        serde_json::from_str::<VersionManifest>(
            &fs::read_to_string(&manifest_path)
                .map_err(|err| format!("No se pudo leer manifest cacheado: {err}"))?,
        )
        .map_err(|err| format!("Manifest cacheado inválido: {err}"))?
    } else {
        let manifest = reqwest::blocking::get(MOJANG_MANIFEST_URL)
            .map_err(|err| format!("No se pudo descargar version_manifest oficial: {err}"))?
            .error_for_status()
            .map_err(|err| format!("version_manifest respondió con error HTTP: {err}"))?
            .json::<VersionManifest>()
            .map_err(|err| format!("No se pudo deserializar version_manifest oficial: {err}"))?;

        fs::write(
            &manifest_path,
            serde_json::to_string_pretty(&manifest).map_err(|err| err.to_string())?,
        )
        .map_err(|err| format!("No se pudo guardar manifest global en cache: {err}"))?;

        manifest
    };

    let version = manifest
        .versions
        .into_iter()
        .find(|entry: &ManifestVersionEntry| entry.id == minecraft_version)
        .ok_or_else(|| {
            format!("No se encontró la versión {minecraft_version} en el manifest oficial.")
        })?;

    ensure_official_url(&version.url)?;

    let version_path = versions_cache.join(format!("{minecraft_version}.json"));
    if !version_path.exists() {
        let response = reqwest::blocking::get(version.url)
            .map_err(|err| format!("No se pudo descargar version.json oficial: {err}"))?
            .error_for_status()
            .map_err(|err| format!("version.json respondió con error HTTP: {err}"))?;

        let version_json = response
            .json::<Value>()
            .map_err(|err| format!("No se pudo deserializar version.json: {err}"))?;

        fs::write(
            &version_path,
            serde_json::to_string_pretty(&version_json).map_err(|err| err.to_string())?,
        )
        .map_err(|err| format!("No se pudo guardar version.json en cache global: {err}"))?;
    }

    let version_json = serde_json::from_str::<Value>(
        &fs::read_to_string(&version_path)
            .map_err(|err| format!("No se pudo leer version.json cacheado: {err}"))?,
    )
    .map_err(|err| format!("version.json cacheado inválido: {err}"))?;

    let instance_version_dir = instance_root
        .join("minecraft")
        .join("versions")
        .join(minecraft_version);
    if !instance_version_dir.exists() {
        fs::create_dir_all(&instance_version_dir)
            .map_err(|err| format!("No se pudo crear version dir local de instancia: {err}"))?;
    }

    let local_version_json = instance_version_dir.join(format!("{minecraft_version}.json"));
    if !local_version_json.exists() {
        fs::copy(&version_path, &local_version_json).map_err(|err| {
            format!(
                "No se pudo copiar version.json cacheado hacia la instancia {}: {err}",
                local_version_json.display()
            )
        })?;
    }

    let local_jar = instance_version_dir.join(format!("{minecraft_version}.jar"));
    if !local_jar.exists() {
        download_client_jar(&version_json, &local_jar)?;
    }

    Ok(version_json)
}

fn ensure_official_url(url: &str) -> AppResult<()> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|err| format!("URL oficial inválida: {url}. Error: {err}"))?;
    let host = parsed.host_str().unwrap_or_default();
    let allowed = [
        MOJANG_MANIFEST_HOST,
        MOJANG_PISTON_META_HOST,
        MOJANG_PISTON_DATA_HOST,
        MOJANG_LIBRARIES_HOST,
    ];
    if !allowed.contains(&host) {
        return Err(format!("URL no oficial bloqueada: {url}"));
    }
    Ok(())
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

    write_placeholder_file(jar_path, "")?;
    let client = build_official_client()?;
    download_with_retry(&client, client_url, jar_path, expected_sha1, false).map(|_| ())
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceStateFile {
    pub version: String,
    pub loader: String,
    pub required_java_major: u32,
    pub created_at: String,
    pub state: String,
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

    let instance_json_path = instance_root.join("instance.json");
    let state_file = InstanceStateFile {
        version: metadata.minecraft_version.clone(),
        loader: metadata.loader.clone(),
        required_java_major: metadata.required_java_major,
        created_at: metadata.created_at.clone(),
        state: metadata.state.clone(),
    };
    fs::write(
        &instance_json_path,
        serde_json::to_string_pretty(&state_file).map_err(|err| err.to_string())?,
    )
    .map_err(|err| {
        format!(
            "No se pudo guardar instance.json en {}: {err}",
            instance_json_path.display()
        )
    })?;

    logs.push(format!(
        "Metadata guardada en {} e instance.json en estado {}.",
        metadata_path.display(),
        metadata.state
    ));
    Ok(())
}
