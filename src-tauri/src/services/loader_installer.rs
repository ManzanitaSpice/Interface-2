use std::{fs, path::Path, process::Command};

use reqwest::blocking::Client;
use serde_json::Value;

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
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(30))
        .user_agent("InterfaceLauncher/0.1")
        .build()
        .map_err(|err| format!("No se pudo crear cliente HTTP para loaders: {err}"))?;

    match normalized_loader.as_str() {
        "fabric" => install_fabric_like(
            &client,
            minecraft_root,
            minecraft_version,
            loader_version,
            "https://meta.fabricmc.net/v2/versions/loader/{minecraft_version}/{loader_version}/profile/json",
            "fabric",
            logs,
        ),
        "quilt" => install_fabric_like(
            &client,
            minecraft_root,
            minecraft_version,
            loader_version,
            "https://meta.quiltmc.org/v3/versions/loader/{minecraft_version}/{loader_version}/profile/json",
            "quilt",
            logs,
        ),
        "forge" => install_forge_like(
            &client,
            minecraft_root,
            minecraft_version,
            loader_version,
            java_exec,
            "https://maven.minecraftforge.net/net/minecraftforge/forge/{minecraft_version}-{loader_version}/forge-{minecraft_version}-{loader_version}-installer.jar",
            "forge",
            logs,
        ),
        "neoforge" => install_forge_like(
            &client,
            minecraft_root,
            minecraft_version,
            loader_version,
            java_exec,
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/{loader_version}/neoforge-{loader_version}-installer.jar",
            "neoforge",
            logs,
        ),
        _ => Err(format!("Loader no soportado todavía: {loader}")),
    }
}

fn install_fabric_like(
    client: &Client,
    minecraft_root: &Path,
    minecraft_version: &str,
    loader_version: &str,
    profile_url_template: &str,
    loader_name: &str,
    logs: &mut Vec<String>,
) -> AppResult<()> {
    let profile_url = profile_url_template
        .replace("{minecraft_version}", minecraft_version)
        .replace("{loader_version}", loader_version);

    let profile = client
        .get(&profile_url)
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

    let mut downloaded = 0_u64;
    for library in profile
        .get("libraries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let artifact = library
            .get("downloads")
            .and_then(|v| v.get("artifact"))
            .cloned();

        let (url, path) = if let Some(artifact) = artifact {
            (
                artifact
                    .get("url")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "download.url faltante".to_string())?
                    .to_string(),
                artifact
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "download.path faltante".to_string())?
                    .to_string(),
            )
        } else {
            continue;
        };

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

    logs.push(format!(
        "Loader {loader_name} instalado: versionId={version_id}, librerías nuevas={downloaded}."
    ));
    Ok(())
}

fn install_forge_like(
    client: &Client,
    minecraft_root: &Path,
    minecraft_version: &str,
    loader_version: &str,
    java_exec: &Path,
    installer_url_template: &str,
    loader_name: &str,
    logs: &mut Vec<String>,
) -> AppResult<()> {
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
    fs::write(&installer_jar, bytes).map_err(|err| {
        format!(
            "No se pudo guardar installer {}: {err}",
            installer_jar.display()
        )
    })?;

    let output = Command::new(java_exec)
        .arg("-jar")
        .arg(&installer_jar)
        .arg("--installClient")
        .arg(minecraft_root)
        .output()
        .map_err(|err| format!("No se pudo ejecutar installer de {loader_name}: {err}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!(
            "Falló installer de {loader_name}. stdout: {} | stderr: {}",
            stdout.trim(),
            stderr.trim()
        ));
    }

    logs.push(format!(
        "Loader {loader_name} instalado vía installer jar (processors ejecutados por installer oficial)."
    ));
    Ok(())
}
