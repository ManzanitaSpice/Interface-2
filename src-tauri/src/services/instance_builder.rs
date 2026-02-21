use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::VecDeque,
    fs,
    path::Path,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, SystemTime},
};

use crate::{
    domain::{
        minecraft::{
            manifest::{ManifestVersionEntry, VersionManifest},
            rule_engine::{evaluate_rules, RuleContext},
        },
        models::instance::InstanceMetadata,
    },
    infrastructure::{
        checksum::sha1::compute_file_sha1,
        downloader::queue::{build_official_client, download_with_retry, DownloadJob},
    },
    services::loader_installer::install_loader_if_needed,
    shared::result::AppResult,
};

const MOJANG_MANIFEST_URL: &str =
    "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json";
const RESOURCES_URL: &str = "https://resources.download.minecraft.net";

#[derive(Debug, Clone)]
pub struct InstanceBuildProgress {
    pub step: String,
    pub step_index: u64,
    pub total_steps: u64,
    pub message: String,
    pub completed: u64,
    pub total: u64,
}

pub fn build_instance_structure(
    instance_root: &Path,
    minecraft_root: &Path,
    minecraft_version: &str,
    loader: &str,
    loader_version: &str,
    java_exec: &Path,
    logs: &mut Vec<String>,
    on_progress: &mut dyn FnMut(InstanceBuildProgress),
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

    fs::create_dir_all(minecraft_root.join("versions"))
        .map_err(|err| format!("No se pudo crear versions/: {err}"))?;

    let shared_libraries = launcher_root.join("libraries");
    let shared_assets = launcher_root.join("assets");
    fs::create_dir_all(&shared_libraries)
        .map_err(|err| format!("No se pudo crear directorio global de libraries: {err}"))?;
    fs::create_dir_all(shared_assets.join("indexes"))
        .map_err(|err| format!("No se pudo crear assets/indexes global: {err}"))?;
    fs::create_dir_all(shared_assets.join("objects"))
        .map_err(|err| format!("No se pudo crear assets/objects global: {err}"))?;

    mirror_shared_dir(&shared_libraries, &minecraft_root.join("libraries"))?;
    mirror_shared_dir(&shared_assets, &minecraft_root.join("assets"))?;

    on_progress(InstanceBuildProgress {
        step: "resolving_manifest".to_string(),
        step_index: 1,
        total_steps: 8,
        message: "Resolviendo version manifest...".to_string(),
        completed: 0,
        total: 1,
    });
    let normalized_minecraft_version = normalize_minecraft_version_id(minecraft_version);
    let version_entry = load_manifest_entry(launcher_root, &normalized_minecraft_version)?;

    on_progress(InstanceBuildProgress {
        step: "downloading_version_json".to_string(),
        step_index: 2,
        total_steps: 8,
        message: "Descargando version.json...".to_string(),
        completed: 0,
        total: 1,
    });
    let version_json = download_version_json(minecraft_root, &version_entry)?;

    on_progress(InstanceBuildProgress {
        step: "downloading_client_jar".to_string(),
        step_index: 3,
        total_steps: 8,
        message: "Descargando client.jar...".to_string(),
        completed: 0,
        total: 1,
    });
    download_client_jar(minecraft_root, &version_entry.id, &version_json)?;

    on_progress(InstanceBuildProgress {
        step: "downloading_libraries".to_string(),
        step_index: 4,
        total_steps: 8,
        message: "Descargando libraries...".to_string(),
        completed: 0,
        total: 1,
    });
    download_libraries(&version_json, &shared_libraries, on_progress)?;

    on_progress(InstanceBuildProgress {
        step: "downloading_assets_index".to_string(),
        step_index: 5,
        total_steps: 8,
        message: "Descargando assets index...".to_string(),
        completed: 0,
        total: 1,
    });
    let assets_index = download_assets_index(&version_json, &shared_assets)?;

    on_progress(InstanceBuildProgress {
        step: "downloading_assets".to_string(),
        step_index: 6,
        total_steps: 8,
        message: "Descargando assets...".to_string(),
        completed: 0,
        total: 1,
    });
    download_assets_objects(&assets_index, &shared_assets, on_progress)?;

    on_progress(InstanceBuildProgress {
        step: "installing_loader".to_string(),
        step_index: 7,
        total_steps: 8,
        message: "Instalando loader...".to_string(),
        completed: 0,
        total: 1,
    });
    let effective_version_id = prepare_loader(
        minecraft_root,
        &normalized_minecraft_version,
        loader,
        loader_version,
        java_exec,
        logs,
    )?;

    on_progress(InstanceBuildProgress {
        step: "persisting_instance_metadata".to_string(),
        step_index: 8,
        total_steps: 8,
        message: "Persistiendo metadata de instancia...".to_string(),
        completed: 1,
        total: 1,
    });

    Ok(effective_version_id)
}

fn normalize_minecraft_version_id(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let lower = trimmed.to_ascii_lowercase();
    for token in ["-neoforge-", "-forge-", "-fabric-loader-", "-quilt-loader-"] {
        if let Some(index) = lower.find(token) {
            return trimmed[..index].to_string();
        }
    }
    trimmed.to_string()
}

fn mirror_shared_dir(shared: &Path, local: &Path) -> AppResult<()> {
    if local.exists() {
        return Ok(());
    }

    #[cfg(unix)]
    {
        if std::os::unix::fs::symlink(shared, local).is_ok() {
            return Ok(());
        }
    }

    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_dir(shared, local).is_ok() {
            return Ok(());
        }
    }

    fs::create_dir_all(local).map_err(|err| {
        format!(
            "No se pudo enlazar/crear directorio local {} hacia {}: {err}",
            local.display(),
            shared.display()
        )
    })
}

fn load_manifest_entry(
    launcher_root: &Path,
    minecraft_version: &str,
) -> AppResult<ManifestVersionEntry> {
    let cache_path = launcher_root.join("cache").join("version_manifest_v2.json");
    if must_refresh_manifest(&cache_path)? {
        let client = build_official_client()?;
        let response = client
            .get(MOJANG_MANIFEST_URL)
            .send()
            .and_then(|res| res.error_for_status())
            .map_err(|err| format!("No se pudo descargar version manifest: {err}"))?;
        let manifest = response
            .text()
            .map_err(|err| format!("No se pudo leer body de version manifest: {err}"))?;
        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("No se pudo crear cache para manifest: {err}"))?;
        }
        fs::write(&cache_path, manifest)
            .map_err(|err| format!("No se pudo guardar manifest en cache: {err}"))?;
    }

    let manifest_raw = fs::read_to_string(&cache_path).map_err(|err| {
        format!(
            "No se pudo leer manifest cacheado {}: {err}",
            cache_path.display()
        )
    })?;
    let manifest = serde_json::from_str::<VersionManifest>(&manifest_raw)
        .map_err(|err| format!("Manifest cacheado inválido: {err}"))?;

    manifest
        .versions
        .into_iter()
        .find(|entry| entry.id == minecraft_version)
        .ok_or_else(|| {
            format!("No se encontró la versión {minecraft_version} en el manifest oficial.")
        })
}

fn must_refresh_manifest(cache_path: &Path) -> AppResult<bool> {
    if !cache_path.exists() {
        return Ok(true);
    }

    let metadata = fs::metadata(cache_path)
        .map_err(|err| format!("No se pudo leer metadata del cache manifest: {err}"))?;
    let modified = metadata
        .modified()
        .map_err(|err| format!("No se pudo leer mtime de cache manifest: {err}"))?;
    let elapsed = SystemTime::now()
        .duration_since(modified)
        .unwrap_or_else(|_| Duration::from_secs(0));
    Ok(elapsed > Duration::from_secs(3600))
}

fn download_version_json(minecraft_root: &Path, entry: &ManifestVersionEntry) -> AppResult<Value> {
    let version_dir = minecraft_root.join("versions").join(&entry.id);
    fs::create_dir_all(&version_dir)
        .map_err(|err| format!("No se pudo crear version dir: {err}"))?;
    let version_json_path = version_dir.join(format!("{}.json", entry.id));

    let client = build_official_client()?;
    let bytes = client
        .get(&entry.url)
        .send()
        .and_then(|res| res.error_for_status())
        .map_err(|err| format!("No se pudo descargar version.json {}: {err}", entry.url))?
        .bytes()
        .map_err(|err| {
            format!(
                "No se pudieron leer bytes de version.json {}: {err}",
                entry.url
            )
        })?;

    fs::write(&version_json_path, &bytes).map_err(|err| {
        format!(
            "No se pudo guardar version.json {}: {err}",
            version_json_path.display()
        )
    })?;

    if let Some(expected_sha1) = &entry.sha1 {
        let sha1 = compute_file_sha1(&version_json_path)?;
        if !sha1.eq_ignore_ascii_case(expected_sha1) {
            return Err(format!(
                "SHA1 inválido para version.json {} (esperado={}, obtenido={}).",
                version_json_path.display(),
                expected_sha1,
                sha1
            ));
        }
    }

    serde_json::from_slice(&bytes).map_err(|err| format!("version.json inválido: {err}"))
}

fn download_client_jar(
    minecraft_root: &Path,
    version_id: &str,
    version_json: &Value,
) -> AppResult<()> {
    let client_url = version_json
        .get("downloads")
        .and_then(|d| d.get("client"))
        .and_then(|d| d.get("url"))
        .and_then(Value::as_str)
        .ok_or_else(|| "version.json no contiene downloads.client.url".to_string())?;
    let expected_sha1 = version_json
        .get("downloads")
        .and_then(|d| d.get("client"))
        .and_then(|d| d.get("sha1"))
        .and_then(Value::as_str)
        .ok_or_else(|| "version.json no contiene downloads.client.sha1".to_string())?;
    let expected_size = version_json
        .get("downloads")
        .and_then(|d| d.get("client"))
        .and_then(|d| d.get("size"))
        .and_then(Value::as_u64)
        .unwrap_or(0);

    let jar_path = minecraft_root
        .join("versions")
        .join(version_id)
        .join(format!("{version_id}.jar"));

    let client = build_official_client()?;
    download_with_retry(&client, client_url, &jar_path, expected_sha1, false)?;

    if expected_size > 0 {
        let current_size = fs::metadata(&jar_path)
            .map_err(|err| format!("No se pudo leer metadata de {}: {err}", jar_path.display()))?
            .len();
        if current_size != expected_size {
            return Err(format!(
                "Tamaño inválido para client.jar {} (esperado={}, obtenido={}).",
                jar_path.display(),
                expected_size,
                current_size
            ));
        }
    }

    Ok(())
}

fn download_libraries(
    version_json: &Value,
    shared_libraries_root: &Path,
    on_progress: &mut dyn FnMut(InstanceBuildProgress),
) -> AppResult<()> {
    let libraries = version_json
        .get("libraries")
        .and_then(Value::as_array)
        .ok_or_else(|| "version.json no contiene libraries[]".to_string())?;

    let rule_context = RuleContext::current();
    let mut jobs = Vec::new();

    for lib in libraries {
        if let Some(rules) = lib.get("rules").and_then(Value::as_array) {
            if !evaluate_rules(rules, &rule_context) {
                continue;
            }
        }

        let artifact = lib.get("downloads").and_then(|d| d.get("artifact"));
        let path = artifact
            .and_then(|a| a.get("path"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        if path.is_empty() {
            continue;
        }

        let expected_sha1 = artifact
            .and_then(|a| a.get("sha1"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        let url = artifact
            .and_then(|a| a.get("url"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("https://libraries.minecraft.net/{path}"));

        jobs.push(DownloadJob {
            url,
            target_path: shared_libraries_root.join(path),
            expected_sha1,
            label: path.to_string(),
        });
    }

    let total = jobs.len() as u64;
    if total == 0 {
        return Ok(());
    }

    run_download_jobs_limited(jobs, 8)?;
    on_progress(InstanceBuildProgress {
        step: "downloading_libraries".to_string(),
        step_index: 4,
        total_steps: 8,
        message: "Descargando libraries...".to_string(),
        completed: total,
        total,
    });
    Ok(())
}

fn download_assets_index(version_json: &Value, shared_assets_root: &Path) -> AppResult<Value> {
    let asset_index = version_json
        .get("assetIndex")
        .ok_or_else(|| "version.json no contiene assetIndex".to_string())?;

    let id = asset_index
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "assetIndex.id faltante".to_string())?;
    let url = asset_index
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| "assetIndex.url faltante".to_string())?;

    let index_path = shared_assets_root
        .join("indexes")
        .join(format!("{id}.json"));
    let client = build_official_client()?;
    let bytes = client
        .get(url)
        .send()
        .and_then(|res| res.error_for_status())
        .map_err(|err| format!("No se pudo descargar assets index {url}: {err}"))?
        .bytes()
        .map_err(|err| format!("No se pudo leer assets index {url}: {err}"))?;

    fs::write(&index_path, &bytes).map_err(|err| {
        format!(
            "No se pudo guardar assets index {}: {err}",
            index_path.display()
        )
    })?;

    serde_json::from_slice(&bytes).map_err(|err| format!("assets index inválido: {err}"))
}

fn download_assets_objects(
    assets_index: &Value,
    shared_assets_root: &Path,
    on_progress: &mut dyn FnMut(InstanceBuildProgress),
) -> AppResult<()> {
    let objects = assets_index
        .get("objects")
        .and_then(Value::as_object)
        .ok_or_else(|| "assets index no contiene objects".to_string())?;

    let mut jobs = Vec::new();
    for obj in objects.values() {
        let hash = obj.get("hash").and_then(Value::as_str).unwrap_or_default();
        if hash.len() < 2 {
            continue;
        }
        let size = obj.get("size").and_then(Value::as_u64).unwrap_or(0);
        let prefix = &hash[0..2];
        let target = shared_assets_root.join("objects").join(prefix).join(hash);
        if target.exists()
            && size > 0
            && fs::metadata(&target).map(|m| m.len()).unwrap_or_default() == size
        {
            continue;
        }
        jobs.push((
            DownloadJob {
                url: format!("{RESOURCES_URL}/{prefix}/{hash}"),
                target_path: target,
                expected_sha1: String::new(),
                label: hash.to_string(),
            },
            size,
        ));
    }

    let total = jobs.len() as u64;
    if total == 0 {
        return Ok(());
    }

    run_download_jobs_limited(jobs.into_iter().map(|(job, _)| job).collect(), 16)?;
    on_progress(InstanceBuildProgress {
        step: "downloading_assets".to_string(),
        step_index: 6,
        total_steps: 8,
        message: "Descargando assets...".to_string(),
        completed: total,
        total,
    });
    Ok(())
}

fn run_download_jobs_limited(jobs: Vec<DownloadJob>, max_concurrency: usize) -> AppResult<()> {
    let workers = max_concurrency.max(1).min(jobs.len().max(1));
    let queue = Arc::new(Mutex::new(VecDeque::from(jobs)));
    let progress = Arc::new(Mutex::new(0_u64));
    let errors = Arc::new(Mutex::new(Vec::<String>::new()));

    thread::scope(|scope| {
        for _ in 0..workers {
            let queue = Arc::clone(&queue);
            let progress = Arc::clone(&progress);
            let errors = Arc::clone(&errors);
            scope.spawn(move || {
                let client = match build_official_client() {
                    Ok(client) => client,
                    Err(err) => {
                        if let Ok(mut e) = errors.lock() {
                            e.push(err);
                        }
                        return;
                    }
                };

                loop {
                    let next = queue.lock().ok().and_then(|mut q| q.pop_front());
                    let Some(job) = next else { break };

                    if let Err(err) = download_with_retry(
                        &client,
                        &job.url,
                        &job.target_path,
                        &job.expected_sha1,
                        false,
                    ) {
                        if let Ok(mut e) = errors.lock() {
                            e.push(format!("{} => {}", job.url, err));
                        }
                        continue;
                    }

                    if let Ok(mut count) = progress.lock() {
                        *count += 1;
                    }
                }
            });
        }
    });

    let errors = errors
        .lock()
        .map_err(|_| "No se pudo bloquear colección de errores de descarga".to_string())?;
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join(" | "))
    }
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
    let effective_loader = if normalized_loader == "quilit" {
        logs.push("Loader 'quilit' detectado; se normaliza automáticamente a 'quilt'.".to_string());
        "quilt"
    } else {
        normalized_loader.as_str()
    };
    if effective_loader.is_empty() || effective_loader == "vanilla" {
        logs.push("Loader VANILLA seleccionado: sin instalación adicional.".to_string());
        return Ok(minecraft_version.to_string());
    }

    let effective = install_loader_if_needed(
        minecraft_root,
        minecraft_version,
        effective_loader,
        loader_version,
        java_exec,
        logs,
    )?;

    logs.push(format!(
        "Loader {} instalado correctamente con version_id efectiva {}.",
        loader, effective
    ));
    Ok(effective)
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
