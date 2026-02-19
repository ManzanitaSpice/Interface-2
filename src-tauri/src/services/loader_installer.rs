use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::SystemTime,
};

use fs2::available_space;
use reqwest::blocking::Client;
use reqwest::header::ACCEPT_ENCODING;
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
) -> AppResult<String> {
    let normalized_loader = loader.trim().to_ascii_lowercase();
    if normalized_loader == "vanilla" || normalized_loader.is_empty() {
        logs.push("Loader vanilla: no se requiere instalación adicional.".to_string());
        return Ok(minecraft_version.to_string());
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
        "neoforge" => install_neoforge(
            minecraft_root,
            minecraft_version,
            loader_version,
            java_exec,
            minecraft_root
                .parent()
                .and_then(Path::parent)
                .ok_or_else(|| {
                    format!(
                        "No se pudo resolver launcher root desde minecraft_root {}",
                        minecraft_root.display()
                    )
                })?,
            logs,
        ),
        _ => Err(format!("Loader no soportado todavía: {loader}")),
    }
}

fn expected_main_class_for_loader(loader: &str) -> Option<&'static str> {
    match loader.trim().to_ascii_lowercase().as_str() {
        "vanilla" | "" => Some("net.minecraft.client.main.Main"),
        "fabric" => Some("net.fabricmc.loader.impl.launch.knot.KnotClient"),
        "forge" | "neoforge" => Some("cpw.mods.bootstraplauncher.BootstrapLauncher"),
        _ => None,
    }
}

fn ensure_loader_main_class(version_json: &mut Value, loader_name: &str) -> Option<String> {
    let expected = expected_main_class_for_loader(loader_name)?;
    let current = version_json
        .get("mainClass")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    if current == expected {
        return None;
    }

    version_json["mainClass"] = Value::String(expected.to_string());
    Some(current)
}

fn install_fabric_like(
    client: &Client,
    minecraft_root: &Path,
    _minecraft_version: &str,
    _loader_version: &str,
    profile_url: &str,
    loader_name: &str,
    logs: &mut Vec<String>,
) -> AppResult<String> {
    let mut profile = client
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

    if let Some(previous_main_class) = ensure_loader_main_class(&mut profile, loader_name) {
        logs.push(format!(
            "Loader {loader_name}: mainClass normalizada (anterior='{}').",
            if previous_main_class.is_empty() {
                "(vacía)"
            } else {
                previous_main_class.as_str()
            }
        ));
    }

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
    Ok(version_id)
}

fn install_forge_legacy(
    client: &Client,
    minecraft_root: &Path,
    minecraft_version: &str,
    loader_version: &str,
    logs: &mut Vec<String>,
) -> AppResult<String> {
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

    if let Some(previous_main_class) = ensure_loader_main_class(&mut version_json, "forge") {
        logs.push(format!(
            "Forge legacy: mainClass normalizada (anterior='{}').",
            if previous_main_class.is_empty() {
                "(vacía)"
            } else {
                previous_main_class.as_str()
            }
        ));
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
    Ok(version_id)
}

fn build_neoforge_installer_url(neoforge_version: &str) -> String {
    format!(
        "https://maven.neoforged.net/releases/net/neoforged/neoforge/{neoforge_version}/neoforge-{neoforge_version}-installer.jar"
    )
}

fn verify_neoforge_preconditions(mc_root: &Path, mc_version: &str) -> AppResult<()> {
    let version_dir = mc_root.join("versions").join(mc_version);
    let vanilla_json = version_dir.join(format!("{mc_version}.json"));
    let vanilla_jar = version_dir.join(format!("{mc_version}.jar"));

    if !vanilla_json.exists() {
        return Err(format!(
            "client.jar de vanilla {mc_version} debe descargarse primero. No existe {}",
            vanilla_json.display()
        ));
    }
    if !vanilla_jar.exists() {
        return Err(format!(
            "version.json de vanilla {mc_version} debe descargarse primero. No existe {}",
            vanilla_jar.display()
        ));
    }

    let file = fs::File::open(&vanilla_jar).map_err(|err| {
        format!(
            "No se pudo abrir client.jar vanilla {}: {err}",
            vanilla_jar.display()
        )
    })?;
    ZipArchive::new(file).map_err(|err| {
        format!(
            "client.jar corrupto, re-descarga vanilla. Archivo {} inválido: {err}",
            vanilla_jar.display()
        )
    })?;

    let free = available_space(mc_root).map_err(|err| {
        format!(
            "No se pudo consultar espacio libre en {}: {err}",
            mc_root.display()
        )
    })?;
    let min = 500_u64 * 1024 * 1024;
    if free < min {
        return Err(format!(
            "Espacio insuficiente en {}. Disponible={} bytes, requerido={} bytes",
            mc_root.display(),
            free,
            min
        ));
    }

    Ok(())
}

fn download_neoforge_installer(
    url: &str,
    launcher_root: &Path,
    logs: &mut Vec<String>,
) -> AppResult<PathBuf> {
    let installers_dir = launcher_root.join("cache").join("installers");
    fs::create_dir_all(&installers_dir).map_err(|err| {
        format!(
            "No se pudo crear directorio cache de installers {}: {err}",
            installers_dir.display()
        )
    })?;

    let file_name = url
        .rsplit('/')
        .next()
        .ok_or_else(|| format!("URL installer inválida: {url}"))?;
    let target = installers_dir.join(file_name);

    if target.exists() {
        let file = fs::File::open(&target).map_err(|err| {
            format!(
                "No se pudo abrir installer cacheado {}: {err}",
                target.display()
            )
        })?;
        ZipArchive::new(file)
            .map_err(|err| format!("Installer cacheado inválido {}: {err}", target.display()))?;
        logs.push(format!(
            "Reutilizando installer cacheado válido: {}",
            target.display()
        ));
        return Ok(target);
    }

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .connect_timeout(std::time::Duration::from_secs(60))
        .user_agent("InterfaceLauncher/0.2")
        .gzip(false)
        .brotli(false)
        .deflate(false)
        .build()
        .map_err(|err| format!("No se pudo crear cliente HTTP NeoForge: {err}"))?;

    let mut response = client
        .get(url)
        .header(ACCEPT_ENCODING, "identity")
        .send()
        .and_then(|res| res.error_for_status())
        .map_err(|err| format!("No se pudo descargar installer NeoForge {url}: {err}"))?;

    let mut file = fs::File::create(&target)
        .map_err(|err| format!("No se pudo crear installer {}: {err}", target.display()))?;
    let mut total = 0_u64;
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = response
            .read(&mut buffer)
            .map_err(|err| format!("Error leyendo stream de installer NeoForge {url}: {err}"))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .map_err(|err| format!("No se pudo escribir chunk en {}: {err}", target.display()))?;
        total += read as u64;
    }

    drop(file);
    let zip_file = fs::File::open(&target).map_err(|err| {
        format!(
            "No se pudo abrir installer descargado {}: {err}",
            target.display()
        )
    })?;
    ZipArchive::new(zip_file).map_err(|err| {
        format!(
            "Installer NeoForge descargado no es ZIP válido en {} ({} bytes): {err}",
            target.display(),
            total
        )
    })?;

    Ok(target)
}

fn run_neoforge_installer(
    java_path: &Path,
    installer_jar: &Path,
    mc_root: &Path,
    logs: &mut Vec<String>,
) -> AppResult<()> {
    let mut cmd = Command::new(java_path);
    cmd.arg("-jar")
        .arg(installer_jar)
        .arg("--installClient")
        .arg(mc_root)
        .current_dir(mc_root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = cmd.output().map_err(|err| {
        format!(
            "No se pudo ejecutar NeoForge installer {} con java {}: {err}",
            installer_jar.display(),
            java_path.display()
        )
    })?;

    let stdout_str = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr_str = String::from_utf8_lossy(&output.stderr).to_string();

    for line in stdout_str
        .lines()
        .rev()
        .take(20)
        .collect::<Vec<_>>()
        .iter()
        .rev()
    {
        logs.push(format!("[neoforge-installer] {line}"));
    }
    for line in stderr_str
        .lines()
        .rev()
        .take(20)
        .collect::<Vec<_>>()
        .iter()
        .rev()
    {
        logs.push(format!("[neoforge-installer][stderr] {line}"));
    }

    if !output.status.success() {
        if stderr_str.to_ascii_lowercase().contains("access is denied")
            || stderr_str.to_ascii_lowercase().contains("acceso denegado")
        {
            logs.push(
                "Posible bloqueo de antivirus. Agrega la carpeta del launcher a las exclusiones."
                    .to_string(),
            );
        }
        return Err(format!(
            "NeoForge installer falló con código {:?}.
STDOUT:
{}
STDERR:
{}",
            output.status.code(),
            stdout_str,
            stderr_str
        ));
    }

    Ok(())
}

fn parse_neoforge_candidate_json(
    versions_dir: &Path,
    version_id: &str,
    mc_version: &str,
) -> Option<(String, SystemTime)> {
    let json_path = versions_dir
        .join(version_id)
        .join(format!("{version_id}.json"));
    let raw = fs::read_to_string(&json_path).ok()?;
    let json = serde_json::from_str::<Value>(&raw).ok()?;
    let inherits = json.get("inheritsFrom").and_then(Value::as_str)?;
    let id = json.get("id").and_then(Value::as_str).unwrap_or(version_id);
    if inherits != mc_version || !id.to_ascii_lowercase().contains("neoforge") {
        return None;
    }
    let modified = fs::metadata(&json_path).and_then(|m| m.modified()).ok()?;
    Some((id.to_string(), modified))
}

fn detect_installed_neoforge_version(
    mc_root: &Path,
    mc_version: &str,
    neoforge_version: &str,
    installer_jar: Option<&Path>,
) -> AppResult<String> {
    let versions_dir = mc_root.join("versions");

    let exact_modern = format!("neoforge-{neoforge_version}");
    let modern_json = versions_dir
        .join(&exact_modern)
        .join(format!("{exact_modern}.json"));
    if modern_json.exists() {
        return Ok(exact_modern);
    }

    let legacy = format!("{mc_version}-neoforge-{neoforge_version}");
    let legacy_json = versions_dir.join(&legacy).join(format!("{legacy}.json"));
    if legacy_json.exists() {
        return Ok(legacy);
    }

    let mut candidates: Vec<(String, SystemTime)> = Vec::new();
    for entry in fs::read_dir(&versions_dir).map_err(|err| {
        format!(
            "No se pudo leer versions dir {} para detectar NeoForge: {err}",
            versions_dir.display()
        )
    })? {
        let entry = entry.map_err(|err| format!("No se pudo iterar versions dir: {err}"))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if let Some(id) = path.file_name().and_then(|n| n.to_str()) {
            if let Some(candidate) = parse_neoforge_candidate_json(&versions_dir, id, mc_version) {
                candidates.push(candidate);
            }
        }
    }
    candidates.sort_by_key(|(_, modified)| *modified);
    if let Some((version_id, _)) = candidates.pop() {
        return Ok(version_id);
    }

    if let Some(installer_jar) = installer_jar {
        let file = fs::File::open(installer_jar).map_err(|err| {
            format!(
                "No se pudo abrir installer jar {} para fallback install_profile.json: {err}",
                installer_jar.display()
            )
        })?;
        let mut zip = ZipArchive::new(file).map_err(|err| {
            format!(
                "Installer NeoForge ZIP inválido {}: {err}",
                installer_jar.display()
            )
        })?;
        let mut install_profile = zip.by_name("install_profile.json").map_err(|err| {
            format!(
                "No se encontró install_profile.json en installer {}: {err}",
                installer_jar.display()
            )
        })?;
        let mut raw = String::new();
        install_profile.read_to_string(&mut raw).map_err(|err| {
            format!(
                "No se pudo leer install_profile.json de {}: {err}",
                installer_jar.display()
            )
        })?;
        let profile = serde_json::from_str::<Value>(&raw).map_err(|err| {
            format!(
                "install_profile.json inválido en {}: {err}",
                installer_jar.display()
            )
        })?;
        if let Some(version_id) = profile.get("version").and_then(Value::as_str) {
            return Ok(version_id.to_string());
        }
        if let Some(version_id) = profile.get("profile").and_then(Value::as_str) {
            return Ok(version_id.to_string());
        }
    }

    Err(format!(
        "No se detectó NeoForge instalado después del installer.

Buscado en: {}

Contenido del directorio:
{}",
        versions_dir.display(),
        list_versions_dir_contents(mc_root)
    ))
}

fn list_versions_dir_contents(mc_root: &Path) -> String {
    let versions_dir = mc_root.join("versions");
    let mut lines = Vec::new();
    let Ok(entries) = fs::read_dir(&versions_dir) else {
        return format!("No se pudo leer {}", versions_dir.display());
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(id) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let json_path = path.join(format!("{id}.json"));
        lines.push(format!(
            "- {id} (json: {})",
            if json_path.exists() { "sí" } else { "no" }
        ));
    }

    if lines.is_empty() {
        "(versions vacío)".to_string()
    } else {
        lines.sort();
        lines.join(
            "
",
        )
    }
}

fn validate_neoforge_version_json(
    mc_root: &Path,
    version_id: &str,
    expected_mc_version: &str,
) -> AppResult<()> {
    let version_json_path = mc_root
        .join("versions")
        .join(version_id)
        .join(format!("{version_id}.json"));
    if !version_json_path.exists() {
        return Err(format!(
            "No existe version.json de NeoForge en {}",
            version_json_path.display()
        ));
    }

    let raw = fs::read_to_string(&version_json_path).map_err(|err| {
        format!(
            "No se pudo leer version.json NeoForge {}: {err}",
            version_json_path.display()
        )
    })?;
    let json = serde_json::from_str::<Value>(&raw).map_err(|err| {
        format!(
            "version.json NeoForge inválido en {}: {err}",
            version_json_path.display()
        )
    })?;

    let inherits = json
        .get("inheritsFrom")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("Falta inheritsFrom en {}", version_json_path.display()))?;
    if inherits != expected_mc_version {
        return Err(format!(
            "inheritsFrom inválido en {}. esperado={}, encontrado={}",
            version_json_path.display(),
            expected_mc_version,
            inherits
        ));
    }

    let main_class = json
        .get("mainClass")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("Falta mainClass en {}", version_json_path.display()))?;
    let main_class_lower = main_class.to_ascii_lowercase();
    if !main_class_lower.contains("bootstraplauncher") && !main_class_lower.contains("cpw.mods") {
        return Err(format!(
            "mainClass inválida en {}. valor={}",
            version_json_path.display(),
            main_class
        ));
    }

    let libraries = json
        .get("libraries")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("Falta array libraries en {}", version_json_path.display()))?;
    if libraries.is_empty() {
        return Err(format!(
            "libraries vacío en {} para version_id={}",
            version_json_path.display(),
            version_id
        ));
    }

    Ok(())
}

fn install_neoforge(
    mc_root: &Path,
    mc_version: &str,
    neoforge_version: &str,
    java_path: &Path,
    launcher_root: &Path,
    logs: &mut Vec<String>,
) -> AppResult<String> {
    let java_major = ensure_neoforge_java(java_path)?;
    ensure_minecraft_layout(mc_root)?;

    logs.push("=== NEOFORGE INSTALL DEBUG ===".to_string());
    logs.push(format!("mc_root: {}", mc_root.display()));
    logs.push(format!("mc_version: {mc_version}"));
    logs.push(format!("neoforge_version: {neoforge_version}"));
    logs.push(format!("java_path: {}", java_path.display()));
    logs.push(format!("java_path exists: {}", java_path.exists()));
    logs.push(format!("mc_root exists: {}", mc_root.exists()));
    logs.push(format!(
        "vanilla jar exists: {}",
        mc_root
            .join("versions")
            .join(mc_version)
            .join(format!("{mc_version}.jar"))
            .exists()
    ));
    logs.push(format!(
        "vanilla json exists: {}",
        mc_root
            .join("versions")
            .join(mc_version)
            .join(format!("{mc_version}.json"))
            .exists()
    ));

    if cfg!(target_os = "windows") {
        let mc_root_len = mc_root.display().to_string().len();
        if mc_root_len > 200 {
            logs.push(format!(
                "[warning] Ruta mc_root larga en Windows ({mc_root_len} chars): {}",
                mc_root.display()
            ));
        }
    }

    logs.push("Verificando precondiciones para NeoForge...".to_string());
    verify_neoforge_preconditions(mc_root, mc_version)?;

    logs.push("Construyendo URL del installer de NeoForge...".to_string());
    let url = build_neoforge_installer_url(neoforge_version);
    logs.push(format!("URL installer: {url}"));

    if let Ok(version_id) =
        detect_installed_neoforge_version(mc_root, mc_version, neoforge_version, None)
    {
        logs.push(format!("NeoForge ya instalado: {version_id}"));
        return Ok(version_id);
    }

    logs.push("Descargando installer de NeoForge...".to_string());
    let installer_path = download_neoforge_installer(&url, launcher_root, logs)?;
    logs.push(format!(
        "Installer descargado: {}",
        installer_path.display()
    ));

    logs.push("Ejecutando NeoForge installer...".to_string());
    logs.push(format!(
        "Comando: {} -jar {} --installClient {} (Java detectado: {java_major})",
        java_path.display(),
        installer_path.display(),
        mc_root.display()
    ));
    let _args = neoforge_installer_args();
    run_neoforge_installer(java_path, &installer_path, mc_root, logs)?;
    logs.push("Installer ejecutado sin errores reportados.".to_string());

    logs.push("Detectando version.json instalado por NeoForge...".to_string());
    let version_id = detect_installed_neoforge_version(
        mc_root,
        mc_version,
        neoforge_version,
        Some(&installer_path),
    )
    .map_err(|err| {
        format!(
            "Installer terminó exitosamente pero no se encontró el version.json.

Esto indica un bug en el installer o permisos insuficientes.

Detalle: {err}"
        )
    })?;
    logs.push(format!("NeoForge instalado con version_id: {version_id}"));

    validate_neoforge_version_json(mc_root, &version_id, mc_version)?;
    logs.push("version.json de NeoForge validado correctamente.".to_string());

    Ok(version_id)
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
) -> AppResult<String> {
    ensure_minecraft_layout(minecraft_root)?;

    let versions_dir = minecraft_root.join("versions");
    if let Some(existing_version_id) = find_existing_loader_version(
        &versions_dir,
        minecraft_version,
        loader_version,
        loader_name,
    )? {
        logs.push(format!(
            "Loader {loader_name} ya disponible en versions como {existing_version_id}. Se omite reinstalación.",
        ));
        return Ok(existing_version_id);
    }

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

    if !installer_jar.exists() {
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
    } else {
        logs.push(format!(
            "Reutilizando installer cacheado para {loader_name}: {}",
            installer_jar.display()
        ));
    }

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

    let mut installed_json = serde_json::from_str::<Value>(
        &fs::read_to_string(&installed_version_json)
            .map_err(|err| format!("No se pudo leer version.json de {loader_name}: {err}"))?,
    )
    .map_err(|err| format!("version.json de {loader_name} inválido: {err}"))?;
    if let Some(previous_main_class) = ensure_loader_main_class(&mut installed_json, loader_name) {
        fs::write(
            &installed_version_json,
            serde_json::to_vec_pretty(&installed_json).map_err(|err| err.to_string())?,
        )
        .map_err(|err| {
            format!(
                "No se pudo persistir mainClass corregida para {loader_name} en {}: {err}",
                installed_version_json.display()
            )
        })?;
        logs.push(format!(
            "Loader {loader_name}: mainClass normalizada tras installer (anterior='{}').",
            if previous_main_class.is_empty() {
                "(vacía)"
            } else {
                previous_main_class.as_str()
            }
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
    Ok(installed_version_id)
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
            is_loader_version_candidate(
                versions_dir,
                id,
                minecraft_version,
                loader_version,
                loader_name,
            )
        })
        .collect::<Vec<_>>();

    if candidates.is_empty() {
        candidates = collect_version_ids(versions_dir)?
            .into_iter()
            .filter(|id| {
                is_loader_version_candidate(
                    versions_dir,
                    id,
                    minecraft_version,
                    loader_version,
                    loader_name,
                )
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

fn find_existing_loader_version(
    versions_dir: &Path,
    minecraft_version: &str,
    loader_version: &str,
    loader_name: &str,
) -> AppResult<Option<String>> {
    let mut candidates = collect_version_ids(versions_dir)?
        .into_iter()
        .filter(|id| {
            is_loader_version_candidate(
                versions_dir,
                id,
                minecraft_version,
                loader_version,
                loader_name,
            )
        })
        .collect::<Vec<_>>();
    candidates.sort();
    Ok(candidates.pop())
}

fn is_loader_version_candidate(
    versions_dir: &Path,
    version_id: &str,
    minecraft_version: &str,
    loader_version: &str,
    loader_name: &str,
) -> bool {
    let lower = version_id.to_ascii_lowercase();
    let loader = loader_name.to_ascii_lowercase();
    if !lower.contains(&loader) {
        return version_json_looks_like_loader(
            versions_dir,
            version_id,
            &loader,
            minecraft_version,
            loader_version,
        );
    }

    if lower.contains(&minecraft_version.to_ascii_lowercase())
        || lower.contains(&loader_version.to_ascii_lowercase())
    {
        return true;
    }

    version_json_looks_like_loader(
        versions_dir,
        version_id,
        &loader,
        minecraft_version,
        loader_version,
    )
}

fn version_json_looks_like_loader(
    versions_dir: &Path,
    version_id: &str,
    loader_name: &str,
    minecraft_version: &str,
    loader_version: &str,
) -> bool {
    let version_json_path = versions_dir
        .join(version_id)
        .join(format!("{version_id}.json"));
    let Ok(raw) = fs::read_to_string(&version_json_path) else {
        return false;
    };
    let Ok(version_json) = serde_json::from_str::<Value>(&raw) else {
        return false;
    };

    let inherits_from = version_json
        .get("inheritsFrom")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let id = version_json
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let libraries_match = version_json
        .get("libraries")
        .and_then(Value::as_array)
        .map(|libraries| {
            libraries.iter().any(|library| {
                let name = library
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                name.contains(loader_name)
                    && (name.contains(&loader_version.to_ascii_lowercase())
                        || name.contains(&minecraft_version.to_ascii_lowercase()))
            })
        })
        .unwrap_or(false);

    (id.contains(loader_name)
        || libraries_match
        || version_id.to_ascii_lowercase().contains(loader_name))
        && (id.contains(&loader_version.to_ascii_lowercase())
            || inherits_from.contains(&minecraft_version.to_ascii_lowercase())
            || libraries_match)
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

        let (path, candidate_urls) = if let Some(artifact) = artifact {
            let path = artifact
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let artifact_url = artifact
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();

            if artifact_url.is_empty() {
                let urls = candidate_maven_urls(library, &path);
                (path, urls)
            } else {
                (path, vec![artifact_url])
            }
        } else if let Some(name) = library.get("name").and_then(Value::as_str) {
            if let Some(path) = maven_name_to_relative_path(name) {
                let urls = candidate_maven_urls(library, &path);
                (path, urls)
            } else {
                continue;
            }
        } else {
            continue;
        };

        if path.is_empty() || candidate_urls.is_empty() {
            continue;
        }

        let target = minecraft_root.join("libraries").join(&path);
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

        let mut last_error = String::new();
        let mut downloaded_bytes = None;
        for url in &candidate_urls {
            match client
                .get(url)
                .send()
                .and_then(|response| response.error_for_status())
            {
                Ok(response) => match response.bytes() {
                    Ok(bytes) => {
                        downloaded_bytes = Some(bytes);
                        break;
                    }
                    Err(err) => {
                        last_error = format!("No se pudo leer bytes de {url}: {err}");
                    }
                },
                Err(err) => {
                    last_error = format!("No se pudo descargar librería {url}: {err}");
                }
            }
        }

        let bytes = downloaded_bytes.ok_or_else(|| {
            format!(
                "No se pudo descargar librería {} desde ninguno de los repos candidatos. Último error: {}",
                path, last_error
            )
        })?;

        fs::write(&target, bytes)
            .map_err(|err| format!("No se pudo guardar librería {}: {err}", target.display()))?;
        downloaded += 1;
    }

    Ok(downloaded)
}

fn candidate_maven_urls(library: &Value, path: &str) -> Vec<String> {
    if path.is_empty() {
        return Vec::new();
    }

    let mut repos = Vec::new();
    if let Some(repo) = library.get("url").and_then(Value::as_str) {
        repos.push(repo.to_string());
    }
    repos.extend([
        "https://libraries.minecraft.net/".to_string(),
        "https://maven.minecraftforge.net/".to_string(),
        "https://maven.neoforged.net/releases/".to_string(),
        "https://repo1.maven.org/maven2/".to_string(),
    ]);

    let mut urls = Vec::new();
    for repo in repos {
        let repo = repo.trim();
        if repo.is_empty() {
            continue;
        }

        let base = if repo.ends_with('/') {
            repo.to_string()
        } else {
            format!("{repo}/")
        };
        let candidate = format!("{base}{path}");
        if !urls.contains(&candidate) {
            urls.push(candidate);
        }
    }

    urls
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
