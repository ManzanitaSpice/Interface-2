use flate2::read::GzDecoder;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    cmp::max,
    ffi::OsStr,
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};
use tar::Archive;
use tauri::{path::BaseDirectory, Manager};
use zip::ZipArchive;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateInstancePayload {
    name: String,
    group: String,
    minecraft_version: String,
    loader: String,
    loader_version: String,
    ram_mb: u32,
    java_args: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateInstanceResult {
    id: String,
    name: String,
    group: String,
    launcher_root: String,
    instance_root: String,
    minecraft_path: String,
    logs: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstanceMetadata {
    name: String,
    group: String,
    minecraft_version: String,
    loader: String,
    loader_version: String,
    ram_mb: u32,
    java_args: Vec<String>,
    java_path: String,
    java_runtime: String,
    last_used: Option<String>,
    internal_uuid: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum JavaRuntime {
    Java8,
    Java17,
    Java21,
}

impl JavaRuntime {
    fn as_dir_name(self) -> &'static str {
        match self {
            JavaRuntime::Java8 => "java8",
            JavaRuntime::Java17 => "java17",
            JavaRuntime::Java21 => "java21",
        }
    }

    fn major(self) -> u8 {
        match self {
            JavaRuntime::Java8 => 8,
            JavaRuntime::Java17 => 17,
            JavaRuntime::Java21 => 21,
        }
    }
}

#[derive(Debug, Deserialize)]
struct AdoptiumPackageChecksum {
    checksum: String,
}

#[derive(Debug, Deserialize)]
struct AdoptiumBinaryPackage {
    link: String,
    checksum: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    checksum_link: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AdoptiumBinary {
    package: AdoptiumBinaryPackage,
}

#[derive(Debug, Deserialize)]
struct AdoptiumRelease {
    binary: AdoptiumBinary,
}

#[tauri::command]
fn create_instance(
    app: tauri::AppHandle,
    payload: CreateInstancePayload,
) -> Result<CreateInstanceResult, String> {
    let mut logs: Vec<String> = Vec::new();

    if payload.name.trim().is_empty() {
        return Err("El nombre de la instancia es obligatorio.".to_string());
    }

    if payload.minecraft_version.trim().is_empty() {
        return Err("La versión de Minecraft es obligatoria.".to_string());
    }

    let launcher_root = resolve_launcher_root(&app)?;
    logs.push(format!("Base launcher: {}", launcher_root.display()));

    create_launcher_directories(&launcher_root, &mut logs)?;

    let required_java = determine_required_java(&payload.minecraft_version, &payload.loader)?;
    logs.push(format!(
        "Java requerido detectado para MC {} + loader {}: Java {}.",
        payload.minecraft_version,
        payload.loader,
        required_java.major()
    ));

    let java_exec = ensure_embedded_java(&launcher_root, required_java, &mut logs)?;

    logs.push(format!(
        "Validando versión seleccionada: {}",
        payload.minecraft_version
    ));
    logs.push(
        "Descargando version_manifest.json (paso preparado para integración real).".to_string(),
    );
    logs.push(
        "Descargando client.jar y version.json (paso preparado para integración real).".to_string(),
    );
    logs.push("Descargando libraries/assets (paso preparado para integración real).".to_string());
    if payload.loader != "vanilla" {
        logs.push(format!(
            "Instalando loader {} {} (paso preparado para integración real).",
            payload.loader, payload.loader_version
        ));
    }

    let sanitized_name = sanitize_path_segment(&payload.name);
    let instance_root = launcher_root.join("instances").join(&sanitized_name);
    let minecraft_root = instance_root.join("minecraft");

    fs::create_dir_all(&instance_root).map_err(|err| err.to_string())?;
    logs.push(format!("Creada carpeta base: {}", instance_root.display()));

    let structure_dirs = [
        instance_root.join("logs"),
        instance_root.join("mods"),
        instance_root.join("config"),
        instance_root.join("resourcepacks"),
        instance_root.join("shaderpacks"),
        instance_root.join("saves"),
        minecraft_root.join("assets"),
        minecraft_root.join("libraries"),
        minecraft_root
            .join("versions")
            .join(&payload.minecraft_version),
        minecraft_root.join("mods"),
        minecraft_root.join("config"),
        minecraft_root.join("logs"),
        minecraft_root.join("crash-reports"),
        minecraft_root.join("saves"),
    ];

    for dir in structure_dirs {
        fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    }
    logs.push("Estructura interna de instancia y .minecraft creada.".to_string());

    let version_file_base = minecraft_root
        .join("versions")
        .join(&payload.minecraft_version);
    let jar_path = version_file_base.join(format!("{}.jar", payload.minecraft_version));
    let json_path = version_file_base.join(format!("{}.json", payload.minecraft_version));

    write_placeholder_file(&jar_path, "placeholder minecraft jar")?;
    write_placeholder_file(
        &json_path,
        &format!(
            "{{\"id\":\"{}\",\"type\":\"release\"}}",
            payload.minecraft_version
        ),
    )?;

    logs.push(
        "Classpath generado (placeholder para implementación del launcher runtime).".to_string(),
    );

    let internal_uuid = uuid::Uuid::new_v4().to_string();
    let metadata = InstanceMetadata {
        name: payload.name,
        group: payload.group,
        minecraft_version: payload.minecraft_version,
        loader: payload.loader,
        loader_version: payload.loader_version,
        ram_mb: payload.ram_mb,
        java_args: payload.java_args,
        java_path: java_exec.display().to_string(),
        java_runtime: required_java.as_dir_name().to_string(),
        last_used: None,
        internal_uuid: internal_uuid.clone(),
    };

    let metadata_path = instance_root.join(".instance.json");
    let metadata_content =
        serde_json::to_string_pretty(&metadata).map_err(|err| err.to_string())?;
    fs::write(&metadata_path, metadata_content).map_err(|err| err.to_string())?;
    logs.push(format!(
        "Metadata guardada en {} (java: {}).",
        metadata_path.display(),
        metadata.java_path
    ));

    Ok(CreateInstanceResult {
        id: internal_uuid,
        name: metadata.name,
        group: metadata.group,
        launcher_root: launcher_root.display().to_string(),
        instance_root: instance_root.display().to_string(),
        minecraft_path: minecraft_root.display().to_string(),
        logs,
    })
}

fn resolve_launcher_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .resolve("InterfaceLauncher", BaseDirectory::AppData)
        .map_err(|err| err.to_string())
}

fn create_launcher_directories(root: &Path, logs: &mut Vec<String>) -> Result<(), String> {
    let dirs = [
        root.join("runtime/java8"),
        root.join("runtime/java17"),
        root.join("runtime/java21"),
        root.join("instances"),
        root.join("assets"),
        root.join("cache"),
        root.join("logs"),
        root.join("config"),
        root.join("versions"),
    ];

    for dir in dirs {
        fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    }

    let launcher_config = root.join("config/launcher.json");
    if !launcher_config.exists() {
        fs::write(
            &launcher_config,
            "{\n  \"defaultPage\": \"Mis Modpacks\",\n  \"javaPath\": \"runtime/java17/bin/java\"\n}\n",
        )
        .map_err(|err| err.to_string())?;
    }

    let accounts_config = root.join("config/accounts.json");
    if !accounts_config.exists() {
        fs::write(&accounts_config, "[]\n").map_err(|err| err.to_string())?;
    }

    logs.push("Estructura global del launcher verificada/creada.".to_string());
    Ok(())
}

fn ensure_embedded_java(
    root: &Path,
    runtime: JavaRuntime,
    logs: &mut Vec<String>,
) -> Result<PathBuf, String> {
    let arch = detect_architecture()?;
    logs.push(format!("Arquitectura detectada: {arch}."));

    let runtime_root = root.join("runtime").join(runtime.as_dir_name());
    let java_exec = java_executable_path(&runtime_root);
    if java_exec.exists() {
        logs.push(format!(
            "Java {} ya instalado: {}",
            runtime.major(),
            java_exec.display()
        ));
        return Ok(java_exec);
    }

    fs::create_dir_all(&runtime_root).map_err(|err| err.to_string())?;
    logs.push(format!(
        "Java {} no encontrado. Iniciando descarga de runtime embebido oficial (Temurin).",
        runtime.major()
    ));

    let client = Client::builder()
        .user_agent("InterfaceLauncher/0.1")
        .build()
        .map_err(|err| format!("No se pudo crear cliente HTTP: {err}"))?;

    let (download_url, expected_checksum, file_name) =
        resolve_temurin_asset(&client, runtime, arch)?;

    logs.push(format!("Descargando: {download_url}"));
    let archive_bytes = client
        .get(&download_url)
        .send()
        .and_then(|resp| resp.error_for_status())
        .map_err(|err| format!("Fallo la descarga del JDK: {err}"))?
        .bytes()
        .map_err(|err| format!("No se pudo leer el binario descargado: {err}"))?
        .to_vec();

    let archive_sha = sha256_hex(&archive_bytes);
    if !archive_sha.eq_ignore_ascii_case(&expected_checksum) {
        return Err(format!(
            "Checksum inválido para Java {}. Esperado: {}, obtenido: {}",
            runtime.major(),
            expected_checksum,
            archive_sha
        ));
    }

    logs.push(format!(
        "Checksum SHA-256 validado para Java {}.",
        runtime.major()
    ));

    extract_archive(&archive_bytes, &file_name, &runtime_root)?;

    if !java_exec.exists() {
        return Err(format!(
            "Se extrajo el runtime de Java {}, pero no se encontró ejecutable en {}",
            runtime.major(),
            java_exec.display()
        ));
    }

    let marker = runtime_root.join(".installed.json");
    fs::write(
        &marker,
        serde_json::json!({
            "runtime": runtime.as_dir_name(),
            "javaMajor": runtime.major(),
            "downloadUrl": download_url,
            "checksum": expected_checksum,
            "archive": file_name,
            "status": "installed"
        })
        .to_string(),
    )
    .map_err(|err| err.to_string())?;

    logs.push(format!(
        "Java {} instalado y marcado como listo en {}.",
        runtime.major(),
        marker.display()
    ));

    Ok(java_exec)
}

fn resolve_temurin_asset(
    client: &Client,
    runtime: JavaRuntime,
    arch: &str,
) -> Result<(String, String, String), String> {
    let os = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "mac"
    } else {
        "linux"
    };

    let image_type = "jdk";
    let jvm_impl = "hotspot";
    let release_type = "ga";
    let vendor = "eclipse";

    let api = format!(
        "https://api.adoptium.net/v3/assets/feature_releases/{}/ga?architecture={}&heap_size=normal&image_type={}&jvm_impl={}&os={}&page=0&page_size=1&project=jdk&release_type={}&sort_method=DEFAULT&sort_order=DESC&vendor={}",
        runtime.major(), arch, image_type, jvm_impl, os, release_type, vendor
    );

    let releases = client
        .get(&api)
        .send()
        .and_then(|resp| resp.error_for_status())
        .map_err(|err| format!("No se pudo consultar catálogo de Temurin: {err}"))?
        .json::<Vec<AdoptiumRelease>>()
        .map_err(|err| format!("Respuesta inválida del catálogo de Temurin: {err}"))?;

    let release = releases.into_iter().next().ok_or_else(|| {
        "No se encontró release de Temurin para el runtime solicitado.".to_string()
    })?;

    let file_name = release.binary.package.name;
    let checksum = if release.binary.package.checksum.is_empty() {
        let checksum_link = release
            .binary
            .package
            .checksum_link
            .ok_or_else(|| "Release sin checksum disponible.".to_string())?;
        client
            .get(&checksum_link)
            .send()
            .and_then(|resp| resp.error_for_status())
            .map_err(|err| format!("No se pudo leer checksum remoto: {err}"))?
            .json::<AdoptiumPackageChecksum>()
            .map_err(|err| format!("Checksum remoto inválido: {err}"))?
            .checksum
    } else {
        release.binary.package.checksum
    };

    Ok((release.binary.package.link, checksum, file_name))
}

fn extract_archive(archive: &[u8], file_name: &str, destination: &Path) -> Result<(), String> {
    let normalized = file_name.to_ascii_lowercase();

    if normalized.ends_with(".zip") {
        extract_zip_archive(archive, destination)
    } else if normalized.ends_with(".tar.gz") || normalized.ends_with(".tgz") {
        let decoder = GzDecoder::new(Cursor::new(archive));
        let mut tar = Archive::new(decoder);
        tar.unpack(destination)
            .map_err(|err| format!("No se pudo extraer tar.gz: {err}"))?;
        flatten_single_top_level_dir(destination)
    } else {
        Err(format!(
            "Formato de archivo no soportado para runtime Java: {file_name}"
        ))
    }
}

fn extract_zip_archive(archive: &[u8], destination: &Path) -> Result<(), String> {
    let reader = Cursor::new(archive);
    let mut zip = ZipArchive::new(reader).map_err(|err| format!("ZIP inválido: {err}"))?;

    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|err| format!("No se pudo leer entrada ZIP: {err}"))?;
        let out_path = match entry.enclosed_name() {
            Some(path) => destination.join(path),
            None => continue,
        };

        if entry.name().ends_with('/') {
            fs::create_dir_all(&out_path).map_err(|err| err.to_string())?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }

        let mut file = fs::File::create(&out_path).map_err(|err| err.to_string())?;
        std::io::copy(&mut entry, &mut file).map_err(|err| err.to_string())?;
    }

    flatten_single_top_level_dir(destination)
}

fn flatten_single_top_level_dir(destination: &Path) -> Result<(), String> {
    let mut entries = fs::read_dir(destination)
        .map_err(|err| err.to_string())?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();

    if entries.len() != 1 || !entries[0].path().is_dir() {
        return Ok(());
    }

    let top_dir = entries.remove(0).path();
    let children = fs::read_dir(&top_dir)
        .map_err(|err| err.to_string())?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();

    for child in children {
        let from = child.path();
        let name = from
            .file_name()
            .and_then(OsStr::to_str)
            .ok_or_else(|| "Ruta inválida al reorganizar runtime Java.".to_string())?;
        let to = destination.join(name);
        fs::rename(from, to).map_err(|err| err.to_string())?;
    }

    fs::remove_dir_all(top_dir).map_err(|err| err.to_string())?;
    Ok(())
}

fn determine_required_java(mc_version: &str, loader: &str) -> Result<JavaRuntime, String> {
    let mc_req = java_for_minecraft(mc_version)?;
    let loader_req = java_for_loader(loader, mc_version)?;
    Ok(max(mc_req, loader_req))
}

fn java_for_loader(loader: &str, mc_version: &str) -> Result<JavaRuntime, String> {
    let loader_lower = loader.trim().to_ascii_lowercase();
    if [
        "vanilla", "forge", "neoforge", "fabric", "quilt", "quilit", "snapshot",
    ]
    .contains(&loader_lower.as_str())
    {
        return java_for_minecraft(mc_version);
    }

    Err(format!("Loader no soportado: {loader}"))
}

fn java_for_minecraft(mc_version: &str) -> Result<JavaRuntime, String> {
    let (major, minor, patch) = parse_mc_version(mc_version)?;

    if major != 1 {
        return Err(format!(
            "Versión de Minecraft no soportada para auto-detección de Java: {mc_version}"
        ));
    }

    if minor <= 16 {
        return Ok(JavaRuntime::Java8);
    }

    if minor <= 20 {
        let p = patch.unwrap_or(0);
        if minor == 20 && p >= 5 {
            return Ok(JavaRuntime::Java21);
        }
        return Ok(JavaRuntime::Java17);
    }

    Ok(JavaRuntime::Java21)
}

fn parse_mc_version(version: &str) -> Result<(u32, u32, Option<u32>), String> {
    let core = version
        .split(['-', ' '])
        .next()
        .ok_or_else(|| format!("Versión inválida: {version}"))?;

    let mut parts = core.split('.');
    let major = parts
        .next()
        .ok_or_else(|| format!("Versión inválida: {version}"))?
        .parse::<u32>()
        .map_err(|_| format!("No se pudo parsear versión de Minecraft: {version}"))?;
    let minor = parts
        .next()
        .ok_or_else(|| format!("Versión inválida: {version}"))?
        .parse::<u32>()
        .map_err(|_| format!("No se pudo parsear versión de Minecraft: {version}"))?;
    let patch = parts.next().and_then(|value| value.parse::<u32>().ok());

    Ok((major, minor, patch))
}

fn detect_architecture() -> Result<&'static str, String> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("x64"),
        "aarch64" => Ok("aarch64"),
        other => Err(format!(
            "Arquitectura no soportada para Java embebido: {other}"
        )),
    }
}

fn java_executable_path(runtime_root: &Path) -> PathBuf {
    if cfg!(target_os = "windows") {
        runtime_root.join("bin").join("java.exe")
    } else {
        runtime_root.join("bin").join("java")
    }
}

fn write_placeholder_file(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    fs::write(path, content).map_err(|err| err.to_string())
}

fn sanitize_path_segment(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == ' ' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim()
        .replace(' ', "-")
        .to_lowercase();

    if sanitized.is_empty() {
        "instance".to_string()
    } else {
        sanitized
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    format!("{digest:x}")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .invoke_handler(tauri::generate_handler![create_instance])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::{determine_required_java, parse_mc_version, JavaRuntime};

    #[test]
    fn mc_version_ranges_map_to_java_versions() {
        assert_eq!(
            determine_required_java("1.16.5", "vanilla").unwrap(),
            JavaRuntime::Java8
        );
        assert_eq!(
            determine_required_java("1.20.4", "fabric").unwrap(),
            JavaRuntime::Java17
        );
        assert_eq!(
            determine_required_java("1.20.5", "quilt").unwrap(),
            JavaRuntime::Java21
        );
        assert_eq!(
            determine_required_java("1.21.4", "neoforge").unwrap(),
            JavaRuntime::Java21
        );
    }

    #[test]
    fn parser_handles_suffixes() {
        assert_eq!(
            parse_mc_version("1.20.1-forge-47.3.0").unwrap(),
            (1, 20, Some(1))
        );
        assert_eq!(parse_mc_version("1.21.4").unwrap(), (1, 21, Some(4)));
    }
}
