use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};

use reqwest::blocking::Client;
use serde_json::Value;
use zip::ZipArchive;

use crate::domain::loaders::{
    fabric::installer::fabric_profile_url,
    forge::installer::{ensure_modern_forge_java, modern_installer_args},
    neoforge::installer::{ensure_neoforge_java, neoforge_installer_args},
    quilt::installer::quilt_profile_url,
};
use crate::shared::result::AppResult;

pub fn install_loader_if_needed(
    minecraft_root: &Path,
    minecraft_version: &str,
    loader: &str,
    loader_version: &str,
    java_exec: &Path,
    logs: &mut Vec<String>,
) -> AppResult<()> {
    let normalized_loader = loader.trim().to_ascii_lowercase();
    if normalized_loader == "vanilla" || normalized_loader.is_empty() {
        logs.push("Loader vanilla: no se requiere instalación adicional.".to_string());
        return Ok(());
    }

    if loader_version.trim().is_empty() {
        return Err(format!(
            "Loader {} requiere loaderVersion y llegó vacío.",
            loader
        ));
    }

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .connect_timeout(std::time::Duration::from_secs(60))
        .user_agent("InterfaceLauncher/0.2")
        .build()
        .map_err(|err| format!("No se pudo crear cliente HTTP para loaders: {err}"))?;

    logs.push(format!(
        "JAVA ejecutado para loader: {}",
        java_exec.display()
    ));

    match normalized_loader.as_str() {
        "fabric" => install_fabric_like(
            &client,
            minecraft_root,
            minecraft_version,
            loader_version,
            &fabric_profile_url(minecraft_version, loader_version),
            "fabric",
            logs,
        ),
        "quilt" => install_fabric_like(
            &client,
            minecraft_root,
            minecraft_version,
            loader_version,
            &quilt_profile_url(minecraft_version, loader_version),
            "quilt",
            logs,
        ),
        "forge" => {
            if is_legacy_forge(minecraft_version) {
                install_forge_legacy(
                    &client,
                    minecraft_root,
                    minecraft_version,
                    loader_version,
                    logs,
                )
            } else {
                install_forge_like_modern(
                    &client,
                    minecraft_root,
                    minecraft_version,
                    loader_version,
                    java_exec,
                    "https://maven.minecraftforge.net/net/minecraftforge/forge/{minecraft_version}-{loader_version}/forge-{minecraft_version}-{loader_version}-installer.jar",
                    &modern_installer_args(minecraft_version, true),
                    ensure_modern_forge_java(java_exec, "Forge")?,
                    "forge",
                    logs,
                )
            }
        }
        "neoforge" => install_forge_like_modern(
            &client,
            minecraft_root,
            minecraft_version,
            loader_version,
            java_exec,
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/{loader_version}/neoforge-{loader_version}-installer.jar",
            &neoforge_installer_args(),
            ensure_neoforge_java(java_exec)?,
            "neoforge",
            logs,
        ),
        _ => Err(format!("Loader no soportado todavía: {loader}")),
    }
}

fn install_fabric_like(
    client: &Client,
    minecraft_root: &Path,
    _minecraft_version: &str,
    _loader_version: &str,
    profile_url: &str,
    loader_name: &str,
    logs: &mut Vec<String>,
) -> AppResult<()> {
    let profile = client
        .get(profile_url)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|err| format!("No se pudo descargar profile de {loader_name}: {err}"))?
        .json::<Value>()
        .map_err(|err| format!("Profile inválido de {loader_name}: {err}"))?;

    let version_id = profile
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("El profile de {loader_name} no contiene id"))?
        .to_string();

    let version_dir = minecraft_root.join("versions").join(&version_id);
    fs::create_dir_all(&version_dir)
        .map_err(|err| format!("No se pudo crear directorio de versión loader: {err}"))?;

    let version_json_path = version_dir.join(format!("{version_id}.json"));
    fs::write(
        &version_json_path,
        serde_json::to_string_pretty(&profile).map_err(|err| err.to_string())?,
    )
    .map_err(|err| {
        format!(
            "No se pudo guardar version.json de {loader_name} en {}: {err}",
            version_json_path.display()
        )
    })?;

    let downloaded = download_libraries_declared(client, minecraft_root, &profile)?;

    logs.push(format!(
        "Loader {loader_name} instalado: versionId={version_id}, inheritsFrom={}, librerías nuevas={downloaded}.",
        profile
            .get("inheritsFrom")
            .and_then(Value::as_str)
            .unwrap_or("(sin inheritsFrom)")
    ));
    Ok(())
}

fn install_forge_legacy(
    client: &Client,
    minecraft_root: &Path,
    minecraft_version: &str,
    loader_version: &str,
    logs: &mut Vec<String>,
) -> AppResult<()> {
    let universal_url = format!(
        "https://maven.minecraftforge.net/net/minecraftforge/forge/{minecraft_version}-{loader_version}/forge-{minecraft_version}-{loader_version}-universal.jar"
    );

    let bytes = client
        .get(&universal_url)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|err| format!("No se pudo descargar universal jar de forge legacy: {err}"))?
        .bytes()
        .map_err(|err| format!("No se pudieron leer bytes del universal jar: {err}"))?;

    let jar_path = minecraft_root
        .join("versions")
        .join(format!("{minecraft_version}-forge-{loader_version}"));
    fs::create_dir_all(&jar_path).map_err(|err| {
        format!(
            "No se pudo crear carpeta de versión forge legacy {}: {err}",
            jar_path.display()
        )
    })?;

    let target_jar = jar_path.join(format!("{minecraft_version}-forge-{loader_version}.jar"));
    fs::write(&target_jar, &bytes).map_err(|err| {
        format!(
            "No se pudo guardar forge universal {}: {err}",
            target_jar.display()
        )
    })?;

    let mut zip = ZipArchive::new(std::io::Cursor::new(bytes.to_vec()))
        .map_err(|err| format!("Universal jar forge inválido: {err}"))?;
    let mut version_json = read_json_from_archive(&mut zip, "version.json")?;

    if version_json.get("inheritsFrom").is_none() {
        version_json["inheritsFrom"] = Value::String(minecraft_version.to_string());
    }

    let version_id = version_json
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or(&format!("{minecraft_version}-forge-{loader_version}"))
        .to_string();

    let version_dir = minecraft_root.join("versions").join(&version_id);
    fs::create_dir_all(&version_dir)
        .map_err(|err| format!("No se pudo crear version dir forge legacy: {err}"))?;

    fs::write(
        version_dir.join(format!("{version_id}.json")),
        serde_json::to_vec_pretty(&version_json).map_err(|err| err.to_string())?,
    )
    .map_err(|err| format!("No se pudo guardar version.json forge legacy: {err}"))?;

    let legacy_jar_target = version_dir.join(format!("{version_id}.jar"));
    if !legacy_jar_target.exists() {
        fs::copy(&target_jar, &legacy_jar_target).map_err(|err| {
            format!(
                "No se pudo copiar jar legacy a versión efectiva {}: {err}",
                legacy_jar_target.display()
            )
        })?;
    }

    let downloaded = download_libraries_declared(client, minecraft_root, &version_json)?;
    logs.push(format!(
        "Forge legacy instalado: versionId={version_id}, librerías nuevas={downloaded}."
    ));
    Ok(())
}

fn install_forge_like_modern(
    client: &Client,
    minecraft_root: &Path,
    minecraft_version: &str,
    loader_version: &str,
    java_exec: &Path,
    installer_url_template: &str,
    installer_args: &[String],
    java_major: u32,
    loader_name: &str,
    logs: &mut Vec<String>,
) -> AppResult<()> {
    ensure_minecraft_layout(minecraft_root)?;

    let installer_url = installer_url_template
        .replace("{minecraft_version}", minecraft_version)
        .replace("{loader_version}", loader_version);

    let installers_dir = minecraft_root.join("installer-artifacts");
    fs::create_dir_all(&installers_dir).map_err(|err| {
        format!(
            "No se pudo crear directorio de installers {}: {err}",
            installers_dir.display()
        )
    })?;

    let installer_jar =
        installers_dir.join(format!("{}-{}-installer.jar", loader_name, loader_version));

    let bytes = client
        .get(&installer_url)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|err| format!("No se pudo descargar installer de {loader_name}: {err}"))?
        .bytes()
        .map_err(|err| format!("No se pudieron leer bytes de installer: {err}"))?;
    fs::write(&installer_jar, &bytes).map_err(|err| {
        format!(
            "No se pudo guardar installer {}: {err}",
            installer_jar.display()
        )
    })?;

    let versions_dir = minecraft_root.join("versions");
    let existing_versions = collect_version_ids(&versions_dir)?;

    logs.push(format!(
        "Ejecutando installer {loader_name} con cwd={} (Java detectado: {java_major}).",
        minecraft_root.display(),
    ));

    let mut command = Command::new(java_exec);
    command.arg("-jar").arg(&installer_jar);
    for arg in installer_args {
        command.arg(arg);
    }

    let output = command
        .current_dir(minecraft_root)
        .output()
        .map_err(|err| {
            format!(
                "No se pudo ejecutar installer {loader_name} con Java embebido {}: {err}",
                java_exec.display()
            )
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !stdout.is_empty() {
        logs.push(format!("Installer {loader_name} stdout: {stdout}"));
    }
    if !stderr.is_empty() {
        logs.push(format!("Installer {loader_name} stderr: {stderr}"));
    }
    logs.push(format!(
        "Installer {loader_name} exit code: {:?}",
        output.status.code()
    ));

    if !output.status.success() {
        return Err(format!(
            "Installer {loader_name} falló. stdout={} stderr={}",
            stdout, stderr
        ));
    }

    let installed_version_id = detect_installed_loader_version(
        &versions_dir,
        &existing_versions,
        minecraft_version,
        loader_version,
        loader_name,
    )?;

    let installed_version_json = versions_dir
        .join(&installed_version_id)
        .join(format!("{installed_version_id}.json"));
    let installed_version_jar = versions_dir
        .join(&installed_version_id)
        .join(format!("{installed_version_id}.jar"));

    if !installed_version_json.exists() {
        return Err(format!(
            "Installer {loader_name} no generó version.json esperado en {}.",
            installed_version_json.display()
        ));
    }

    if !installed_version_jar.exists() {
        logs.push(format!(
            "Aviso {loader_name}: no se encontró jar de versión en {} (algunos installers modernos usan only-metadata).",
            installed_version_jar.display()
        ));
    }

    logs.push(format!(
        "Loader {loader_name} moderno instalado con installer oficial (--installClient): versionId={installed_version_id}."
    ));
    Ok(())
}

fn ensure_minecraft_layout(minecraft_root: &Path) -> AppResult<()> {
    let required_dirs = [
        minecraft_root.to_path_buf(),
        minecraft_root.join("libraries"),
        minecraft_root.join("versions"),
    ];

    for dir in required_dirs {
        fs::create_dir_all(&dir).map_err(|err| {
            format!(
                "No se pudo preparar directorio requerido para installer en {}: {err}",
                dir.display()
            )
        })?;
    }

    Ok(())
}

fn collect_version_ids(versions_dir: &Path) -> AppResult<Vec<String>> {
    if !versions_dir.exists() {
        return Ok(Vec::new());
    }

    let mut ids = Vec::new();
    for entry in fs::read_dir(versions_dir).map_err(|err| {
        format!(
            "No se pudo leer directorio versions {}: {err}",
            versions_dir.display()
        )
    })? {
        let entry = entry.map_err(|err| format!("No se pudo iterar versions: {err}"))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|value| value.to_str()) {
            ids.push(name.to_string());
        }
    }
    Ok(ids)
}

fn detect_installed_loader_version(
    versions_dir: &Path,
    previous_ids: &[String],
    minecraft_version: &str,
    loader_version: &str,
    loader_name: &str,
) -> AppResult<String> {
    let previous_set = previous_ids
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    let mut candidates = collect_version_ids(versions_dir)?
        .into_iter()
        .filter(|id| !previous_set.contains(id))
        .filter(|id| {
            is_loader_version_candidate(id, minecraft_version, loader_version, loader_name)
        })
        .collect::<Vec<_>>();

    if candidates.is_empty() {
        candidates = collect_version_ids(versions_dir)?
            .into_iter()
            .filter(|id| {
                is_loader_version_candidate(id, minecraft_version, loader_version, loader_name)
            })
            .collect();
    }

    candidates.sort();
    candidates.pop().ok_or_else(|| {
        format!(
            "No se pudo detectar versión instalada para loader={loader_name}, mc={minecraft_version}, loaderVersion={loader_version} en {}",
            versions_dir.display()
        )
    })
}

fn is_loader_version_candidate(
    version_id: &str,
    minecraft_version: &str,
    loader_version: &str,
    loader_name: &str,
) -> bool {
    let lower = version_id.to_ascii_lowercase();
    let loader = loader_name.to_ascii_lowercase();
    lower.contains(&loader)
        && (lower.contains(&minecraft_version.to_ascii_lowercase())
            || lower.contains(&loader_version.to_ascii_lowercase()))
}

fn is_legacy_forge(mc_version: &str) -> bool {
    let parts = mc_version
        .split('.')
        .filter_map(|part| part.parse::<u32>().ok())
        .collect::<Vec<_>>();
    if parts.len() < 2 {
        return false;
    }
    let minor = parts[1];
    minor <= 12
}

fn read_json_from_archive(
    zip: &mut ZipArchive<std::io::Cursor<Vec<u8>>>,
    name: &str,
) -> AppResult<Value> {
    let mut file = zip
        .by_name(name)
        .map_err(|err| format!("No se encontró {name} dentro del installer: {err}"))?;
    let mut raw = String::new();
    file.read_to_string(&mut raw)
        .map_err(|err| format!("No se pudo leer {name} del installer: {err}"))?;
    serde_json::from_str(&raw).map_err(|err| format!("JSON inválido en {name}: {err}"))
}

fn find_zip_entry_bytes<F>(
    zip: &mut ZipArchive<std::io::Cursor<Vec<u8>>>,
    predicate: F,
) -> AppResult<Option<Vec<u8>>>
where
    F: Fn(&str) -> bool,
{
    for idx in 0..zip.len() {
        let mut file = zip
            .by_index(idx)
            .map_err(|err| format!("No se pudo iterar installer zip: {err}"))?;
        let name = file.name().to_string();
        if predicate(&name) {
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)
                .map_err(|err| format!("No se pudo leer entrada {name}: {err}"))?;
            return Ok(Some(bytes));
        }
    }
    Ok(None)
}

fn download_libraries_declared(
    client: &Client,
    minecraft_root: &Path,
    payload: &Value,
) -> AppResult<u64> {
    let libraries = payload
        .get("libraries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    download_libraries_list(client, minecraft_root, &libraries)
}

fn download_libraries_list(
    client: &Client,
    minecraft_root: &Path,
    libraries: &[Value],
) -> AppResult<u64> {
    let mut downloaded = 0_u64;
    for library in libraries {
        let artifact = library
            .get("downloads")
            .and_then(|v| v.get("artifact"))
            .cloned();

        let (url, path) = if let Some(artifact) = artifact {
            (
                artifact
                    .get("url")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                artifact
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            )
        } else if let Some(name) = library.get("name").and_then(Value::as_str) {
            if let Some(path) = maven_name_to_relative_path(name) {
                (format!("https://libraries.minecraft.net/{path}"), path)
            } else {
                continue;
            }
        } else {
            continue;
        };

        if url.is_empty() || path.is_empty() {
            continue;
        }

        let target = minecraft_root.join("libraries").join(path);
        if target.exists() {
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "No se pudo crear directorio de librería {}: {err}",
                    parent.display()
                )
            })?;
        }

        let bytes = client
            .get(&url)
            .send()
            .and_then(|response| response.error_for_status())
            .map_err(|err| format!("No se pudo descargar librería {url}: {err}"))?
            .bytes()
            .map_err(|err| format!("No se pudo leer bytes de {url}: {err}"))?;

        fs::write(&target, bytes)
            .map_err(|err| format!("No se pudo guardar librería {}: {err}", target.display()))?;
        downloaded += 1;
    }

    Ok(downloaded)
}

fn maven_name_to_relative_path(name: &str) -> Option<String> {
    let mut parts = name.split(':');
    let group = parts.next()?;
    let artifact = parts.next()?;
    let version = parts.next()?;
    let classifier = parts.next();

    let base = format!(
        "{}/{}/{}/{}-{}",
        group.replace('.', "/"),
        artifact,
        version,
        artifact,
        version
    );

    Some(match classifier {
        Some(classifier_and_ext) if classifier_and_ext.contains('@') => {
            let (classifier, ext) = classifier_and_ext.split_once('@')?;
            format!("{base}-{classifier}.{ext}")
        }
        Some(classifier) => format!("{base}-{classifier}.jar"),
        None => format!("{base}.jar"),
    })
}

fn maven_name_to_path(minecraft_root: &Path, name: &str) -> Option<PathBuf> {
    let relative = maven_name_to_relative_path(name)?;
    Some(minecraft_root.join("libraries").join(relative))
}
