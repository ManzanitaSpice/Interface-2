use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Mutex, OnceLock},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use zip::ZipArchive;

use crate::{
    app::instance_service::{get_instance_metadata, StartInstanceResult},
    domain::{
        minecraft::{
            argument_resolver::{resolve_launch_arguments, LaunchContext},
            rule_engine::{evaluate_rules, RuleContext, RuleFeatures},
        },
        models::{instance::LaunchAuthSession, java::JavaRuntime},
    },
    services::java_installer::ensure_embedded_java,
};

#[derive(Debug, Clone)]
pub struct RedirectLaunchContext {
    pub version_json_path: PathBuf,
    pub version_json: serde_json::Value,
    pub game_dir: PathBuf,
    pub versions_dir: PathBuf,
    pub libraries_dir: PathBuf,
    pub assets_dir: PathBuf,
    pub minecraft_jar: PathBuf,
    pub launcher_name: String,
}

#[derive(Debug, Clone)]
struct CachedRedirectContext {
    ctx: RedirectLaunchContext,
    version_mtime_ms: u128,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShortcutRedirect {
    source_path: String,
    source_launcher: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RedirectValidationResult {
    pub valid: bool,
    pub source_exists: bool,
    pub version_json_found: bool,
    pub version_json_path: Option<String>,
    pub minecraft_jar_found: bool,
    pub minecraft_jar_path: Option<String>,
    pub java_available: bool,
    pub java_path: Option<String>,
    pub searched_paths: Vec<String>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

static REDIRECT_CTX_CACHE: OnceLock<Mutex<HashMap<String, CachedRedirectContext>>> =
    OnceLock::new();

fn redirect_ctx_cache() -> &'static Mutex<HashMap<String, CachedRedirectContext>> {
    REDIRECT_CTX_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn read_redirect_file(instance_root: &Path) -> Result<ShortcutRedirect, String> {
    let path = instance_root.join(".redirect.json");
    let raw = fs::read_to_string(&path)
        .map_err(|err| format!("No se pudo leer {}: {err}", path.display()))?;
    serde_json::from_str(&raw)
        .map_err(|err| format!("No se pudo parsear {}: {err}", path.display()))
}

fn system_minecraft_root() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|p| p.join(".minecraft"))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|p| p.join("Library/Application Support/minecraft"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|p| p.join(".minecraft"))
    }
}

fn known_launcher_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    #[cfg(target_os = "windows")]
    {
        if let Some(user_profile) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
            roots.push(user_profile.join("curseforge/minecraft/Install"));
        }
        if let Some(app_data) = std::env::var_os("APPDATA").map(PathBuf::from) {
            roots.push(app_data.join("PrismLauncher"));
            roots.push(app_data.join("com.modrinth.theseus/.minecraft"));
            roots.push(app_data.join("MultiMC"));
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            roots.push(home.join("Library/Application Support/PrismLauncher"));
            roots.push(home.join("Library/Application Support/com.modrinth.theseus/.minecraft"));
            roots.push(home.join("Library/Application Support/MultiMC"));
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            roots.push(home.join(".local/share/PrismLauncher"));
            roots.push(home.join(".local/share/ModrinthApp/.minecraft"));
            roots.push(home.join(".local/share/MultiMC"));
            roots.push(home.join(".local/share/curseforge/minecraft/Install"));
        }
    }

    roots
}

fn launcher_roots_for_source(source_launcher: &str) -> Vec<PathBuf> {
    let all = known_launcher_roots();
    if source_launcher
        .trim()
        .eq_ignore_ascii_case("Auto detectado")
    {
        return all;
    }

    let launcher = source_launcher.to_ascii_lowercase();
    all.into_iter()
        .filter(|root| {
            let path = root.to_string_lossy().to_ascii_lowercase();
            (launcher.contains("curseforge") && path.contains("curseforge"))
                || (launcher.contains("prism") && path.contains("prism"))
                || (launcher.contains("modrinth") && path.contains("modrinth"))
                || (launcher.contains("multimc") && path.contains("multimc"))
        })
        .collect()
}

fn unique_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in paths {
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

fn find_version_json(
    source_path: &Path,
    version_id: &str,
    source_launcher: &str,
) -> Option<PathBuf> {
    version_json_candidates(source_path, version_id, source_launcher)
        .into_iter()
        .find(|p| p.exists())
}

fn find_minecraft_jar(
    source_path: &Path,
    version_id: &str,
    source_launcher: &str,
) -> Option<PathBuf> {
    minecraft_jar_candidates(source_path, version_id, source_launcher)
        .into_iter()
        .find(|p| p.exists())
}

fn find_libraries_dir(source_path: &Path, source_launcher: &str) -> Option<PathBuf> {
    libraries_dir_candidates(source_path, source_launcher)
        .into_iter()
        .find(|p| p.exists())
}

fn find_assets_dir(source_path: &Path, source_launcher: &str) -> Option<PathBuf> {
    assets_dir_candidates(source_path, source_launcher)
        .into_iter()
        .find(|p| p.exists())
}

fn version_json_candidates(
    source_path: &Path,
    version_id: &str,
    source_launcher: &str,
) -> Vec<PathBuf> {
    let mut candidates = vec![
        source_path
            .join(".minecraft/versions")
            .join(version_id)
            .join(format!("{version_id}.json")),
        source_path
            .join("versions")
            .join(version_id)
            .join(format!("{version_id}.json")),
        source_path
            .join("minecraft/versions")
            .join(version_id)
            .join(format!("{version_id}.json")),
    ];

    if let Some(system_root) = system_minecraft_root() {
        candidates.push(
            system_root
                .join("versions")
                .join(version_id)
                .join(format!("{version_id}.json")),
        );
    }

    for launcher_root in launcher_roots_for_source(source_launcher) {
        candidates.push(
            launcher_root
                .join("versions")
                .join(version_id)
                .join(format!("{version_id}.json")),
        );
    }

    unique_paths(candidates)
}

fn minecraft_jar_candidates(
    source_path: &Path,
    version_id: &str,
    source_launcher: &str,
) -> Vec<PathBuf> {
    let mut candidates = vec![
        source_path
            .join(".minecraft/versions")
            .join(version_id)
            .join(format!("{version_id}.jar")),
        source_path
            .join("versions")
            .join(version_id)
            .join(format!("{version_id}.jar")),
        source_path
            .join("minecraft/versions")
            .join(version_id)
            .join(format!("{version_id}.jar")),
    ];

    if let Some(system_root) = system_minecraft_root() {
        candidates.push(
            system_root
                .join("versions")
                .join(version_id)
                .join(format!("{version_id}.jar")),
        );
    }

    for launcher_root in launcher_roots_for_source(source_launcher) {
        candidates.push(
            launcher_root
                .join("versions")
                .join(version_id)
                .join(format!("{version_id}.jar")),
        );
    }

    unique_paths(candidates)
}

fn libraries_dir_candidates(source_path: &Path, source_launcher: &str) -> Vec<PathBuf> {
    let mut candidates = vec![
        source_path.join("libraries"),
        source_path.join(".minecraft/libraries"),
        source_path.join("minecraft/libraries"),
    ];

    if let Some(system_root) = system_minecraft_root() {
        candidates.push(system_root.join("libraries"));
    }

    for launcher_root in launcher_roots_for_source(source_launcher) {
        candidates.push(launcher_root.join("libraries"));
    }

    unique_paths(candidates)
}

fn assets_dir_candidates(source_path: &Path, source_launcher: &str) -> Vec<PathBuf> {
    let mut candidates = vec![
        source_path.join("assets"),
        source_path.join(".minecraft/assets"),
        source_path.join("minecraft/assets"),
    ];

    if let Some(system_root) = system_minecraft_root() {
        candidates.push(system_root.join("assets"));
    }

    for launcher_root in launcher_roots_for_source(source_launcher) {
        candidates.push(launcher_root.join("assets"));
    }

    unique_paths(candidates)
}

pub fn resolve_redirect_launch_context(
    source_path: &Path,
    version_id: &str,
    source_launcher: &str,
) -> Result<RedirectLaunchContext, String> {
    if !source_path.exists() {
        return Err(format!("La carpeta original de la instancia ya no existe en: {}. Es posible que el launcher externo haya movido o eliminado la instancia.", source_path.display()));
    }

    let cache_key = format!("{}::{version_id}", source_path.display());
    if let Ok(cache) = redirect_ctx_cache().lock() {
        if let Some(cached) = cache.get(&cache_key) {
            if let Ok(meta) = fs::metadata(&cached.ctx.version_json_path) {
                if let Ok(mtime) = meta.modified() {
                    if mtime
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_millis())
                        .unwrap_or(0)
                        == cached.version_mtime_ms
                    {
                        return Ok(cached.ctx.clone());
                    }
                }
            }
        }
    }

    let version_json_path = find_version_json(source_path, version_id, source_launcher)
        .ok_or_else(|| format!("No se encontró el archivo de versión {version_id}.json. Asegúrate de que la versión esté instalada en {source_launcher} o en el launcher oficial de Mojang."))?;

    let version_raw = fs::read_to_string(&version_json_path).map_err(|err| {
        format!(
            "No se pudo leer version.json {}: {err}",
            version_json_path.display()
        )
    })?;
    let version_json: Value = serde_json::from_str(&version_raw).map_err(|err| {
        format!(
            "No se pudo parsear version.json {}: {err}",
            version_json_path.display()
        )
    })?;

    let minecraft_jar_candidates =
        minecraft_jar_candidates(source_path, version_id, source_launcher);
    let minecraft_jar =
        find_minecraft_jar(source_path, version_id, source_launcher).ok_or_else(|| {
            format!(
                "No se encontró {version_id}.jar. Se buscó en:
{}

Asegúrate de que la versión esté completamente instalada en el launcher de origen.",
                minecraft_jar_candidates
                    .iter()
                    .map(|p| format!("- {}", p.display()))
                    .collect::<Vec<_>>()
                    .join(
                        "
"
                    )
            )
        })?;

    let versions_dir = minecraft_jar
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "No se pudo resolver versions_dir para instancia atajo.".to_string())?
        .to_path_buf();

    let libraries_dir = find_libraries_dir(source_path, source_launcher)
        .ok_or_else(|| "No se encontró carpeta libraries para instancia REDIRECT.".to_string())?;

    let assets_dir = find_assets_dir(source_path, source_launcher)
        .ok_or_else(|| "No se encontró carpeta assets para instancia REDIRECT.".to_string())?;

    let game_dir = if source_path.join(".minecraft").exists() {
        source_path.join(".minecraft")
    } else {
        source_path.to_path_buf()
    };

    let ctx = RedirectLaunchContext {
        version_json_path: version_json_path.clone(),
        version_json,
        game_dir,
        versions_dir,
        libraries_dir,
        assets_dir,
        minecraft_jar,
        launcher_name: source_launcher.to_string(),
    };

    if let Ok(meta) = fs::metadata(&version_json_path) {
        if let Ok(mtime) = meta.modified() {
            if let Ok(mut cache) = redirect_ctx_cache().lock() {
                cache.insert(
                    cache_key,
                    CachedRedirectContext {
                        ctx: ctx.clone(),
                        version_mtime_ms: mtime
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.as_millis())
                            .unwrap_or(0),
                    },
                );
            }
        }
    }

    Ok(ctx)
}

fn parse_java_runtime_for_redirect(version_json: &Value, version_id: &str) -> JavaRuntime {
    let declared = version_json
        .get("javaVersion")
        .and_then(|v| v.get("majorVersion"))
        .and_then(Value::as_u64)
        .unwrap_or(0);

    let major = if declared > 0 {
        declared as u32
    } else {
        let minor = version_id
            .split('.')
            .nth(1)
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(20);
        if minor < 17 {
            8
        } else if minor == 17 {
            16
        } else {
            21
        }
    };

    match major {
        0..=11 => JavaRuntime::Java8,
        12..=20 => JavaRuntime::Java17,
        _ => JavaRuntime::Java21,
    }
}

fn maven_library_path(name: &str) -> Option<PathBuf> {
    let mut parts = name.split(':');
    let group = parts.next()?;
    let artifact = parts.next()?;
    let version = parts.next()?;
    let classifier = parts.next();

    let mut path = PathBuf::new();
    for p in group.split('.') {
        path.push(p);
    }
    path.push(artifact);
    path.push(version);

    let file = if let Some(classifier) = classifier {
        format!("{artifact}-{version}-{classifier}.jar")
    } else {
        format!("{artifact}-{version}.jar")
    };
    path.push(file);
    Some(path)
}

pub fn build_classpath(
    version_json: &serde_json::Value,
    libraries_dir: &Path,
    versions_dir: &Path,
    version_id: &str,
) -> Result<String, String> {
    let sep = if cfg!(target_os = "windows") {
        ";"
    } else {
        ":"
    };
    let ctx = RuleContext::current();
    let mut entries = Vec::new();

    for library in version_json
        .get("libraries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let rules = library
            .get("rules")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if !rules.is_empty() && !evaluate_rules(&rules, &ctx) {
            continue;
        }

        let Some(name) = library.get("name").and_then(Value::as_str) else {
            continue;
        };

        if let Some(relative) = maven_library_path(name) {
            let full = libraries_dir.join(relative);
            if full.exists() {
                entries.push(full.display().to_string());
            } else {
                log::warn!(
                    "Library faltante: {name}. La instancia puede no funcionar correctamente."
                );
            }
        }
    }

    let main_jar = versions_dir
        .join(version_id)
        .join(format!("{version_id}.jar"));
    if !main_jar.exists() {
        return Err(format!("No se encontró {version_id}.jar. La versión puede no estar completamente instalada en Minecraft."));
    }
    entries.push(main_jar.display().to_string());

    Ok(entries.join(sep))
}

fn native_classifier(lib: &Value) -> Option<String> {
    let map = lib.get("natives")?.as_object()?;
    let key = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    };
    let raw = map.get(key)?.as_str()?.to_string();
    let arch = if std::env::consts::ARCH.contains("64") {
        "64"
    } else {
        "32"
    };
    Some(raw.replace("${arch}", arch))
}

pub fn extract_natives(
    version_json: &serde_json::Value,
    libraries_dir: &Path,
    natives_dir: &Path,
) -> Result<(), String> {
    fs::create_dir_all(natives_dir)
        .map_err(|err| format!("No se pudo crear carpeta natives temporal: {err}"))?;

    for library in version_json
        .get("libraries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let Some(name) = library.get("name").and_then(Value::as_str) else {
            continue;
        };

        let Some(classifier) = native_classifier(&library) else {
            continue;
        };

        let native_name = format!("{name}:{classifier}");
        let Some(rel) = maven_library_path(&native_name) else {
            continue;
        };
        let native_jar = libraries_dir.join(rel);
        if !native_jar.exists() {
            continue;
        }

        let file = fs::File::open(&native_jar)
            .map_err(|err| format!("No se pudo abrir native {}: {err}", native_jar.display()))?;
        let mut zip = ZipArchive::new(file)
            .map_err(|err| format!("Native ZIP inválido {}: {err}", native_jar.display()))?;

        for i in 0..zip.len() {
            let mut entry = zip
                .by_index(i)
                .map_err(|err| format!("No se pudo leer entrada native: {err}"))?;
            let name = entry.name().to_string();
            if name.contains("META-INF") || name.ends_with('/') {
                continue;
            }

            let out = natives_dir.join(Path::new(&name).file_name().unwrap_or_default());
            let mut out_file = fs::File::create(&out).map_err(|err| {
                format!("No se pudo crear archivo native {}: {err}", out.display())
            })?;
            std::io::copy(&mut entry, &mut out_file)
                .map_err(|err| format!("No se pudo extraer native {}: {err}", out.display()))?;
        }
    }

    Ok(())
}

#[tauri::command]
pub fn validate_redirect_instance(
    instance_path: String,
) -> Result<RedirectValidationResult, String> {
    let mut warnings = Vec::new();
    let mut errors = Vec::new();
    let instance_root = PathBuf::from(&instance_path);

    let metadata = get_instance_metadata(instance_path.clone())?;
    let redirect = read_redirect_file(&instance_root)?;
    let source = PathBuf::from(&redirect.source_path);
    let source_exists = source.exists();

    let mut version_json_found = false;
    let mut version_json_path = None;
    let mut minecraft_jar_found = false;
    let mut minecraft_jar_path = None;
    let mut java_available = false;
    let mut java_path = None;
    let mut searched_paths: Vec<String> = Vec::new();

    if !source_exists {
        errors.push(format!("La carpeta original de la instancia ya no existe en: {}. Es posible que el launcher externo haya movido o eliminado la instancia.", source.display()));
    } else {
        searched_paths =
            minecraft_jar_candidates(&source, &metadata.version_id, &redirect.source_launcher)
                .into_iter()
                .map(|p| p.display().to_string())
                .collect();

        match resolve_redirect_launch_context(
            &source,
            &metadata.version_id,
            &redirect.source_launcher,
        ) {
            Ok(ctx) => {
                version_json_found = true;
                version_json_path = Some(ctx.version_json_path.display().to_string());
                minecraft_jar_found = true;
                minecraft_jar_path = Some(ctx.minecraft_jar.display().to_string());
                let runtime =
                    parse_java_runtime_for_redirect(&ctx.version_json, &metadata.version_id);
                let launcher_root =
                    instance_root
                        .parent()
                        .and_then(Path::parent)
                        .ok_or_else(|| {
                            "No se pudo resolver launcher_root para validar atajo.".to_string()
                        })?;
                match ensure_embedded_java(launcher_root, runtime, &mut warnings) {
                    Ok(exec) => {
                        java_available = exec.exists();
                        java_path = Some(exec.display().to_string());
                    }
                    Err(_) => {
                        errors.push(format!("Se requiere Java {} para esta instancia. Verifica que el runtime esté descargado en el launcher.", runtime.major()));
                    }
                }
            }
            Err(err) => errors.push(err),
        }
    }

    Ok(RedirectValidationResult {
        valid: errors.is_empty(),
        source_exists,
        version_json_found,
        version_json_path,
        minecraft_jar_found,
        minecraft_jar_path,
        java_available,
        java_path,
        searched_paths,
        warnings,
        errors,
    })
}

pub fn launch_redirect_instance(
    app: AppHandle,
    instance_root: String,
    auth_session: LaunchAuthSession,
) -> Result<StartInstanceResult, String> {
    let metadata = get_instance_metadata(instance_root.clone())?;
    let instance_path = PathBuf::from(&instance_root);
    let redirect = read_redirect_file(&instance_path)?;

    let source_path = PathBuf::from(&redirect.source_path);
    let _ = app.emit(
        "redirect_launch_status",
        json!({
            "stage":"resolving",
            "message":"Buscando archivos de Minecraft...",
            "instance_uuid": metadata.internal_uuid,
            "source_launcher": redirect.source_launcher,
            "exit_code": Value::Null,
            "error": Value::Null
        }),
    );

    let ctx = resolve_redirect_launch_context(
        &source_path,
        &metadata.version_id,
        &redirect.source_launcher,
    )?;
    let runtime = parse_java_runtime_for_redirect(&ctx.version_json, &metadata.version_id);

    let launcher_root = instance_path
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "No se pudo resolver launcher_root para atajo REDIRECT.".to_string())?;

    let mut logs = Vec::new();
    let java_exec = ensure_embedded_java(launcher_root, runtime, &mut logs).map_err(|_| {
        format!(
            "Se requiere Java {} para esta instancia. Verifica que el runtime esté descargado en el launcher.",
            runtime.major()
        )
    })?;

    let classpath = build_classpath(
        &ctx.version_json,
        &ctx.libraries_dir,
        &ctx.versions_dir,
        &metadata.version_id,
    )?;
    let natives_dir = app
        .path()
        .app_cache_dir()
        .map_err(|err| format!("No se pudo resolver cache del launcher: {err}"))?
        .join("natives")
        .join(format!("{}-{}", metadata.internal_uuid, now_millis()));

    let _ = app.emit(
        "redirect_launch_status",
        json!({
            "stage":"extracting_natives",
            "message":"Extrayendo librerías nativas...",
            "instance_uuid": metadata.internal_uuid,
            "source_launcher": redirect.source_launcher,
            "exit_code": Value::Null,
            "error": Value::Null
        }),
    );
    extract_natives(&ctx.version_json, &ctx.libraries_dir, &natives_dir)?;

    let asset_index = ctx
        .version_json
        .get("assetIndex")
        .and_then(|v| v.get("id"))
        .and_then(Value::as_str)
        .or(ctx.version_json.get("assets").and_then(Value::as_str))
        .unwrap_or("legacy")
        .to_string();

    let launch_context = LaunchContext {
        classpath,
        classpath_separator: if cfg!(target_os = "windows") {
            ";"
        } else {
            ":"
        }
        .to_string(),
        library_directory: ctx.libraries_dir.display().to_string(),
        natives_dir: natives_dir.display().to_string(),
        launcher_name: "Interface-2".to_string(),
        launcher_version: env!("CARGO_PKG_VERSION").to_string(),
        auth_player_name: auth_session.profile_name.clone(),
        auth_uuid: auth_session.profile_id.clone(),
        auth_access_token: auth_session.minecraft_access_token.clone(),
        user_type: "msa".to_string(),
        user_properties: "{}".to_string(),
        version_name: metadata.version_id.clone(),
        game_directory: ctx.game_dir.display().to_string(),
        assets_root: ctx.assets_dir.display().to_string(),
        assets_index_name: asset_index,
        version_type: "release".to_string(),
        resolution_width: "1280".to_string(),
        resolution_height: "720".to_string(),
        clientid: String::new(),
        auth_xuid: String::new(),
        xuid: String::new(),
        quick_play_singleplayer: String::new(),
        quick_play_multiplayer: String::new(),
        quick_play_realms: String::new(),
        quick_play_path: String::new(),
    };

    let resolved = resolve_launch_arguments(
        &ctx.version_json,
        &launch_context,
        &RuleContext {
            os_name: RuleContext::current().os_name,
            arch: std::env::consts::ARCH.to_string(),
            features: RuleFeatures::default(),
        },
    )?;

    let mut jvm_args = vec![
        format!("-Xmx{}M", metadata.ram_mb.max(512)),
        "-Xms512M".to_string(),
    ];
    jvm_args.extend(resolved.jvm);
    jvm_args.extend(metadata.java_args.clone());

    let _ = app.emit(
        "redirect_launch_status",
        json!({
            "stage":"launching",
            "message":"Iniciando Minecraft REDIRECT...",
            "instance_uuid": metadata.internal_uuid,
            "source_launcher": redirect.source_launcher,
            "exit_code": Value::Null,
            "error": Value::Null
        }),
    );

    let mut child = Command::new(&java_exec)
        .args(&jvm_args)
        .arg(resolved.main_class.clone())
        .args(&resolved.game)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .current_dir(&ctx.game_dir)
        .spawn()
        .map_err(|err| {
            let message = format!("No se pudo iniciar el proceso REDIRECT: {err}");
            let _ = app.emit(
                "redirect_launch_status",
                json!({
                    "stage":"error",
                    "message":"Falló el inicio de la instancia REDIRECT.",
                    "instance_uuid": metadata.internal_uuid,
                    "source_launcher": redirect.source_launcher,
                    "exit_code": Value::Null,
                    "error": message,
                }),
            );
            message
        })?;

    let pid = child.id();
    let _ = app.emit(
        "redirect_launch_status",
        json!({
            "stage":"running",
            "message":"Instancia REDIRECT en ejecución.",
            "instance_uuid": metadata.internal_uuid,
            "source_launcher": redirect.source_launcher,
            "exit_code": Value::Null,
            "error": Value::Null
        }),
    );

    let app_for_thread = app.clone();
    let instance_uuid = metadata.internal_uuid.clone();
    let source_launcher = redirect.source_launcher.clone();
    thread::spawn(move || {
        let exit_code = child.wait().ok().and_then(|status| status.code());
        let _ = app_for_thread.emit(
            "redirect_launch_status",
            json!({
                "stage":"closed",
                "message":"Instancia REDIRECT finalizada.",
                "instance_uuid": instance_uuid,
                "source_launcher": source_launcher,
                "exit_code": exit_code,
                "error": Value::Null,
            }),
        );
        let _ = fs::remove_dir_all(&natives_dir);
    });

    Ok(StartInstanceResult {
        pid,
        java_path: java_exec.display().to_string(),
        logs,
        refreshed_auth_session: auth_session,
    })
}
