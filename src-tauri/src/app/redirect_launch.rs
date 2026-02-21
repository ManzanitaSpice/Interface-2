use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Mutex, OnceLock},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use zip::ZipArchive;

use tokio::{io::AsyncWriteExt, time::sleep};

use crate::{
    app::instance_service::{get_instance_metadata, StartInstanceResult},
    domain::{
        minecraft::{
            argument_resolver::{resolve_launch_arguments, LaunchContext},
            rule_engine::{evaluate_rules, OsName, RuleContext, RuleFeatures},
        },
        models::{instance::LaunchAuthSession, java::JavaRuntime},
    },
    infrastructure::downloader::queue::{
        ensure_official_binary_url, explain_network_error, official_retries, official_timeout,
    },
    services::java_installer::ensure_embedded_java,
};

const DEFAULT_CACHE_EXPIRY_DAYS: u32 = 7;
const MAX_CACHE_SIZE_MB: u64 = 2048;
const MAX_CACHE_ENTRIES: usize = 10;
const MOJANG_MANIFEST_URL: &str =
    "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json";

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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeOutputEvent {
    instance_root: String,
    stream: String,
    line: String,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedirectCacheEntry {
    pub instance_uuid: String,
    pub version_id: String,
    pub source_path: String,
    pub source_launcher: String,
    pub created_at: String,
    pub last_used_at: String,
    pub expires_after_days: u32,
    pub size_bytes: u64,
    pub complete: bool,
    pub version_json_cached: bool,
    pub jar_cached: bool,
    pub libraries_cached: bool,
    pub assets_cached: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RedirectCacheIndex {
    pub entries: Vec<RedirectCacheEntry>,
    pub total_size_bytes: u64,
    pub last_cleanup_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheCleanupResult {
    pub entries_removed: usize,
    pub bytes_freed: u64,
    pub entries_remaining: usize,
    pub total_size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RedirectCacheInfo {
    pub entries: Vec<RedirectCacheEntryInfo>,
    pub total_size_bytes: u64,
    pub total_size_mb: u64,
    pub max_size_mb: u64,
    pub entry_count: usize,
    pub max_entries: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RedirectCacheEntryInfo {
    pub instance_uuid: String,
    pub version_id: String,
    pub source_launcher: String,
    pub last_used_at: String,
    pub expires_in_days: i64,
    pub size_mb: u64,
    pub complete: bool,
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

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn parse_rfc3339(raw: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|d| d.with_timezone(&chrono::Utc))
}

fn redirect_cache_root(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_cache_dir()
        .map_err(|err| format!("No se pudo resolver app_cache_dir: {err}"))?
        .join("redirect-cache"))
}

fn redirect_cache_index_path(cache_root: &Path) -> PathBuf {
    cache_root.join("meta.json")
}

fn load_redirect_cache_index(cache_root: &Path) -> RedirectCacheIndex {
    let path = redirect_cache_index_path(cache_root);
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<RedirectCacheIndex>(&raw).ok())
        .unwrap_or_default()
}

fn save_redirect_cache_index(cache_root: &Path, index: &RedirectCacheIndex) -> Result<(), String> {
    fs::create_dir_all(cache_root).map_err(|err| format!("No se pudo crear cache root: {err}"))?;
    let raw = serde_json::to_string_pretty(index)
        .map_err(|err| format!("No se pudo serializar índice redirect-cache: {err}"))?;
    fs::write(redirect_cache_index_path(cache_root), raw)
        .map_err(|err| format!("No se pudo guardar índice redirect-cache: {err}"))
}

fn entry_cache_dir(cache_root: &Path, instance_uuid: &str) -> PathBuf {
    cache_root.join(instance_uuid)
}

fn folder_size_bytes(root: &Path) -> u64 {
    let mut total = 0_u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(read) = fs::read_dir(&current) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(meta) = entry.metadata() {
                total = total.saturating_add(meta.len());
            }
        }
    }
    total
}

fn recalc_cache_totals(index: &mut RedirectCacheIndex) {
    index.total_size_bytes = index.entries.iter().map(|e| e.size_bytes).sum::<u64>();
}

fn remove_cache_entry(cache_root: &Path, index: &mut RedirectCacheIndex, instance_uuid: &str) {
    let dir = entry_cache_dir(cache_root, instance_uuid);
    let _ = fs::remove_dir_all(dir);
    index
        .entries
        .retain(|entry| entry.instance_uuid != instance_uuid);
}

fn entry_expired(entry: &RedirectCacheEntry) -> bool {
    let Some(last_used) = parse_rfc3339(&entry.last_used_at) else {
        return true;
    };
    let age = chrono::Utc::now() - last_used;
    age.num_days() > entry.expires_after_days as i64
}

fn run_redirect_cache_cleanup(
    cache_root: &Path,
    index: &mut RedirectCacheIndex,
) -> CacheCleanupResult {
    let before_size = index.total_size_bytes;
    let before_count = index.entries.len();

    let expired_or_invalid: Vec<String> = index
        .entries
        .iter()
        .filter(|entry| {
            entry_expired(entry)
                || !Path::new(&entry.source_path).exists()
                || !entry.complete
                || !entry_cache_dir(cache_root, &entry.instance_uuid).exists()
        })
        .map(|entry| entry.instance_uuid.clone())
        .collect();

    for instance_uuid in expired_or_invalid {
        remove_cache_entry(cache_root, index, &instance_uuid);
    }

    index.entries.sort_by_key(|entry| {
        parse_rfc3339(&entry.last_used_at)
            .map(|d| d.timestamp())
            .unwrap_or(i64::MIN)
    });

    let max_bytes = MAX_CACHE_SIZE_MB * 1024 * 1024;
    recalc_cache_totals(index);
    while index.total_size_bytes > max_bytes || index.entries.len() > MAX_CACHE_ENTRIES {
        let Some(oldest) = index.entries.first().cloned() else {
            break;
        };
        remove_cache_entry(cache_root, index, &oldest.instance_uuid);
        recalc_cache_totals(index);
    }

    index.last_cleanup_at = now_rfc3339();
    recalc_cache_totals(index);

    CacheCleanupResult {
        entries_removed: before_count.saturating_sub(index.entries.len()),
        bytes_freed: before_size.saturating_sub(index.total_size_bytes),
        entries_remaining: index.entries.len(),
        total_size_bytes: index.total_size_bytes,
    }
}

pub fn cleanup_redirect_cache_on_startup(app: &AppHandle) -> Result<(), String> {
    let cache_root = redirect_cache_root(app)?;
    let mut index = load_redirect_cache_index(&cache_root);
    run_redirect_cache_cleanup(&cache_root, &mut index);
    save_redirect_cache_index(&cache_root, &index)
}

pub fn cleanup_redirect_cache_after_launch(app: &AppHandle) -> Result<(), String> {
    cleanup_redirect_cache_on_startup(app)
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

fn read_and_validate_version_json(path: &Path) -> Option<Value> {
    let raw = fs::read_to_string(path).ok()?;
    let json: Value = serde_json::from_str(&raw).ok()?;

    let has_main_class = json.get("mainClass").and_then(Value::as_str).is_some();
    let has_libraries = json.get("libraries").and_then(Value::as_array).is_some();
    let has_arguments = json.get("arguments").is_some() || json.get("minecraftArguments").is_some();

    if !has_main_class || !has_libraries || !has_arguments {
        log::warn!(
            "[REDIRECT] version.json en {} está incompleto, descartando",
            path.display()
        );
        return None;
    }

    Some(json)
}

fn detect_source_instance_hints(source_path: &Path) -> (Option<String>, Option<String>) {
    let prism_manifest = source_path.join("minecraftinstance.json");
    if let Ok(raw) = fs::read_to_string(&prism_manifest) {
        if let Ok(json) = serde_json::from_str::<Value>(&raw) {
            let mc = json
                .get("components")
                .and_then(Value::as_array)
                .and_then(|components| {
                    components.iter().find_map(|component| {
                        let uid = component.get("uid")?.as_str()?;
                        if uid == "net.minecraft" {
                            component
                                .get("version")
                                .and_then(Value::as_str)
                                .map(str::to_string)
                        } else {
                            None
                        }
                    })
                });
            let loader = json
                .get("components")
                .and_then(Value::as_array)
                .and_then(|components| {
                    components.iter().find_map(|component| {
                        let uid = component.get("uid")?.as_str()?.to_ascii_lowercase();
                        if uid.contains("fabric") {
                            Some("fabric".to_string())
                        } else if uid.contains("neoforge") {
                            Some("neoforge".to_string())
                        } else if uid.contains("forge") {
                            Some("forge".to_string())
                        } else if uid.contains("quilt") {
                            Some("quilt".to_string())
                        } else {
                            None
                        }
                    })
                });
            return (loader, mc);
        }
    }

    let modrinth_manifest = source_path.join("profile.json");
    if let Ok(raw) = fs::read_to_string(&modrinth_manifest) {
        if let Ok(json) = serde_json::from_str::<Value>(&raw) {
            let loader = json
                .get("loader")
                .and_then(Value::as_str)
                .map(str::to_ascii_lowercase);
            let mc = json
                .get("game_version")
                .and_then(Value::as_str)
                .map(str::to_string);
            return (loader, mc);
        }
    }

    (None, None)
}

fn score_version_json_candidate(
    path: &Path,
    version_id: &str,
    loader_hint: Option<&str>,
    mc_hint: Option<&str>,
) -> usize {
    let mut score = 0usize;
    let lower = path.to_string_lossy().to_ascii_lowercase();
    let version_lower = version_id.to_ascii_lowercase();

    if lower.contains(&version_lower) {
        score += 30;
    }

    if let Some(loader) = loader_hint {
        if lower.contains(loader) {
            score += 35;
        }
    }

    if let Some(mc) = mc_hint {
        if lower.contains(&mc.to_ascii_lowercase()) {
            score += 18;
        }
    }

    if lower.contains(".minecraft") || lower.contains("minecraft/") {
        score += 8;
    }

    if lower.contains("versions") {
        score += 4;
    }

    score
}

fn resolve_official_version_json(
    version_id: &str,
    source_path: &Path,
    source_launcher: &str,
) -> Result<(PathBuf, Value), String> {
    let (loader_hint, mc_hint) = detect_source_instance_hints(source_path);

    let local_candidates = [
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

    let mut candidate_files = Vec::new();
    candidate_files.extend(local_candidates);

    for launcher_root in launcher_roots_for_source(source_launcher) {
        candidate_files.push(
            launcher_root
                .join("versions")
                .join(version_id)
                .join(format!("{version_id}.json")),
        );
    }

    if let Some(system_root) = system_minecraft_root() {
        candidate_files.push(
            system_root
                .join("versions")
                .join(version_id)
                .join(format!("{version_id}.json")),
        );
    }

    let mut best: Option<(usize, PathBuf, Value)> = None;
    for candidate in unique_paths(candidate_files) {
        if !candidate.is_file() {
            continue;
        }
        let Some(json) = read_and_validate_version_json(&candidate) else {
            continue;
        };
        let score = score_version_json_candidate(
            &candidate,
            version_id,
            loader_hint.as_deref(),
            mc_hint.as_deref(),
        );

        if best
            .as_ref()
            .map(|(best_score, _, _)| score > *best_score)
            .unwrap_or(true)
        {
            best = Some((score, candidate, json));
        }
    }

    if let Some((_, path, json)) = best {
        log::info!(
            "[REDIRECT] Usando version.json resuelto para {}: {}",
            source_launcher,
            path.display()
        );
        return Ok((path, json));
    }

    Err(format!(
        "No se encontró version.json válido para {version_id}. Verifica que la versión esté instalada en {source_launcher} o en el launcher oficial de Mojang."
    ))
}

fn resolve_redirect_game_dir(source_path: &Path) -> PathBuf {
    let preferred = [
        source_path.join("minecraft"),
        source_path.join(".minecraft"),
    ]
    .into_iter()
    .find(|candidate| candidate.is_dir())
    .unwrap_or_else(|| source_path.to_path_buf());
    log::info!(
        "[REDIRECT] game_dir seleccionado (carpeta detectada completa): {}",
        preferred.display()
    );
    preferred
}

fn verify_game_dir_has_instance_data(game_dir: &Path) -> Vec<String> {
    let mut warnings = Vec::new();
    let checks = [
        (
            "mods",
            "No se encontró carpeta mods — la instancia puede no tener mods o la ruta es incorrecta",
        ),
        (
            "options.txt",
            "No se encontró options.txt — las opciones del jugador no se cargarán",
        ),
        (
            "config",
            "No se encontró carpeta config — las configuraciones de mods no se cargarán",
        ),
    ];

    for (relative, warning_msg) in checks {
        let full = game_dir.join(relative);
        if !full.exists() {
            log::warn!("[REDIRECT] {warning_msg}: {}", full.display());
            warnings.push(warning_msg.to_string());
        }
    }

    let found_items = [
        "mods",
        "saves",
        "resourcepacks",
        "shaderpacks",
        "config",
        "options.txt",
    ]
    .iter()
    .filter(|item| game_dir.join(item).exists())
    .map(|item| item.to_string())
    .collect::<Vec<_>>();

    log::info!(
        "[REDIRECT] Datos encontrados en game_dir {}: {:?}",
        game_dir.display(),
        found_items
    );

    warnings
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
    let mut candidates = Vec::new();

    candidates.extend([
        source_path.join("libraries"),
        source_path.join(".minecraft/libraries"),
        source_path.join("minecraft/libraries"),
    ]);

    if let Some(system_root) = system_minecraft_root() {
        candidates.push(system_root.join("libraries"));
    }

    for launcher_root in launcher_roots_for_source(source_launcher) {
        candidates.push(launcher_root.join("libraries"));
    }

    unique_paths(candidates)
}

fn assets_dir_candidates(source_path: &Path, source_launcher: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    candidates.extend([
        source_path.join("assets"),
        source_path.join(".minecraft/assets"),
        source_path.join("minecraft/assets"),
    ]);

    if let Some(system_root) = system_minecraft_root() {
        candidates.push(system_root.join("assets"));
    }

    for launcher_root in launcher_roots_for_source(source_launcher) {
        candidates.push(launcher_root.join("assets"));
    }

    unique_paths(candidates)
}

fn find_minecraft_jar(
    source_path: &Path,
    version_id: &str,
    source_launcher: &str,
) -> Option<PathBuf> {
    minecraft_jar_candidates(source_path, version_id, source_launcher)
        .into_iter()
        .find(|candidate| candidate.is_file())
}

fn find_libraries_dir(source_path: &Path, source_launcher: &str) -> Option<PathBuf> {
    libraries_dir_candidates(source_path, source_launcher)
        .into_iter()
        .find(|candidate| candidate.is_dir())
}

fn find_assets_dir(source_path: &Path, source_launcher: &str) -> Option<PathBuf> {
    assets_dir_candidates(source_path, source_launcher)
        .into_iter()
        .find(|candidate| candidate.is_dir())
}

pub fn resolve_redirect_launch_context(
    source_path: &Path,
    version_id: &str,
    source_launcher: &str,
) -> Result<RedirectLaunchContext, String> {
    log::info!(
        "[REDIRECT] Iniciando resolución para version_id: {}",
        version_id
    );
    log::info!("[REDIRECT] source_path: {}", source_path.display());
    log::info!("[REDIRECT] source_launcher: {}", source_launcher);

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

    let (version_json_path, version_json) =
        resolve_official_version_json(version_id, source_path, source_launcher)?;

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
    log::info!(
        "[REDIRECT] libraries_dir resuelto: {}",
        libraries_dir.display()
    );

    let assets_dir = find_assets_dir(source_path, source_launcher)
        .ok_or_else(|| "No se encontró carpeta assets para instancia REDIRECT.".to_string())?;

    let game_dir = resolve_redirect_game_dir(source_path);
    for warning in verify_game_dir_has_instance_data(&game_dir) {
        log::warn!("[REDIRECT] Advertencia game_dir: {warning}");
    }

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

    let normalized = entries
        .into_iter()
        .map(|entry| {
            if cfg!(target_os = "windows") {
                entry.replace('/', "\\")
            } else {
                entry
            }
        })
        .collect::<Vec<_>>();

    Ok(normalized.join(sep))
}

fn current_os_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "windows"
    }
    #[cfg(target_os = "macos")]
    {
        "macos"
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        "linux"
    }
}

fn current_arch_name() -> &'static str {
    #[cfg(target_arch = "x86_64")]
    {
        "x86_64"
    }
    #[cfg(target_arch = "aarch64")]
    {
        "aarch64"
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        "x86"
    }
}

fn library_rules_allow(library: &Value, current_os: &str, _current_arch: &str) -> bool {
    let Some(rules) = library.get("rules").and_then(Value::as_array) else {
        return true;
    };

    let mut allowed = false;
    for rule in rules {
        let action = rule
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("allow");
        let os_matches = if let Some(os) = rule.get("os") {
            let os_name = os.get("name").and_then(Value::as_str).unwrap_or_default();
            let normalized_current = if current_os == "macos" {
                "osx"
            } else {
                current_os
            };
            os_name.is_empty() || os_name == normalized_current
        } else {
            true
        };

        if os_matches {
            allowed = action == "allow";
        }
    }

    allowed
}

async fn verify_sha1(path: &Path, expected_sha1: &str) -> Result<bool, String> {
    if expected_sha1.is_empty() {
        return Ok(true);
    }
    let actual = crate::infrastructure::checksum::sha1::compute_file_sha1(path)?;
    Ok(actual.eq_ignore_ascii_case(expected_sha1))
}

async fn extract_zip_excluding(
    jar_path: &Path,
    dest_dir: &Path,
    excludes: Vec<String>,
) -> Result<usize, String> {
    let jar_path = jar_path.to_path_buf();
    let dest_dir = dest_dir.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<usize, String> {
        let file = fs::File::open(&jar_path)
            .map_err(|e| format!("No se pudo abrir JAR {}: {e}", jar_path.display()))?;
        let mut archive = ZipArchive::new(file)
            .map_err(|e| format!("No se pudo leer ZIP {}: {e}", jar_path.display()))?;
        let mut extracted = 0usize;

        for i in 0..archive.len() {
            let mut entry = archive
                .by_index(i)
                .map_err(|e| format!("Error leyendo entrada ZIP: {e}"))?;
            let name = entry.name().to_string();

            if excludes.iter().any(|ex| name.starts_with(ex)) || name.ends_with('/') {
                continue;
            }

            let dest_path = dest_dir.join(&name);
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent).map_err(|e| {
                    format!("No se pudo crear directorio {}: {e}", parent.display())
                })?;
            }
            let mut dest_file = fs::File::create(&dest_path)
                .map_err(|e| format!("No se pudo crear archivo {}: {e}", dest_path.display()))?;
            std::io::copy(&mut entry, &mut dest_file)
                .map_err(|e| format!("No se pudo copiar {name}: {e}"))?;
            extracted = extracted.saturating_add(1);
        }

        Ok(extracted)
    })
    .await
    .map_err(|e| format!("Error en spawn_blocking al extraer ZIP: {e}"))?
}

async fn find_or_download_artifact(
    relative_path: &str,
    url: Option<&str>,
    sha1: Option<&str>,
    libraries_dir: &Path,
    cache_dir: &Path,
    source_launcher: &str,
) -> Result<PathBuf, String> {
    let normalized = relative_path.replace('/', std::path::MAIN_SEPARATOR_STR);
    let mut search_paths = vec![libraries_dir.join(&normalized)];

    if let Some(system_root) = system_minecraft_root() {
        search_paths.push(system_root.join("libraries").join(&normalized));
    }

    for root in launcher_roots_for_source(source_launcher) {
        search_paths.push(root.join("libraries").join(&normalized));
    }
    search_paths.push(cache_dir.join("libraries").join(&normalized));

    for candidate in unique_paths(search_paths) {
        log::info!(
            "[REDIRECT]   candidato: {} — existe: {}",
            candidate.display(),
            candidate.exists()
        );
        if !candidate.exists() {
            continue;
        }

        if let Some(expected_sha1) = sha1 {
            if verify_sha1(&candidate, expected_sha1).await? {
                log::info!(
                    "[REDIRECT] Library encontrada y verificada: {}",
                    candidate.display()
                );
                return Ok(candidate);
            }
            log::warn!(
                "[REDIRECT] Library encontrada pero sha1 no coincide: {}",
                candidate.display()
            );
            continue;
        }

        log::info!("[REDIRECT] Library encontrada: {}", candidate.display());
        return Ok(candidate);
    }

    let download_url = url.ok_or_else(|| {
        format!("Library no encontrada y sin URL para descargar: {relative_path}")
    })?;
    let dest = cache_dir.join("libraries").join(&normalized);
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("No se pudo crear directorio para library: {e}"))?;
    }

    log::info!("[REDIRECT] Descargando native desde: {download_url}");
    log::info!("[REDIRECT] Destino descarga: {}", dest.display());
    let client = build_async_official_client()?;
    let _ = download_async_with_retry(
        &client,
        download_url,
        &dest,
        sha1.unwrap_or_default(),
        false,
    )
    .await?;
    Ok(dest)
}

async fn find_or_download_library(
    library: &Value,
    classifier_key: &str,
    libraries_dir: &Path,
    cache_dir: &Path,
    source_launcher: &str,
) -> Result<PathBuf, String> {
    let classifier = library
        .get("downloads")
        .and_then(|d| d.get("classifiers"))
        .and_then(|c| c.get(classifier_key))
        .ok_or_else(|| format!("No hay datos de classifier '{classifier_key}'"))?;
    let relative_path = classifier
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("No hay 'path' para classifier '{classifier_key}'"))?;
    let url = classifier.get("url").and_then(Value::as_str);
    let sha1 = classifier.get("sha1").and_then(Value::as_str);

    log::info!("[REDIRECT] Buscando JAR nativo: {}", relative_path);
    find_or_download_artifact(
        relative_path,
        url,
        sha1,
        libraries_dir,
        cache_dir,
        source_launcher,
    )
    .await
}

async fn extract_single_native(
    library: &Value,
    libraries_dir: &Path,
    cache_dir: &Path,
    natives_dir: &Path,
    current_os: &str,
    current_arch: &str,
    source_launcher: &str,
) -> Result<usize, String> {
    if let Some(natives_map) = library.get("natives") {
        let os_key = if current_os == "macos" {
            "osx"
        } else {
            current_os
        };
        let classifier_raw = natives_map
            .get(os_key)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("No hay native para OS {os_key}"))?;
        let classifier_key = classifier_raw.replace(
            "${arch}",
            if current_arch == "x86_64" { "64" } else { "32" },
        );

        let jar_path = find_or_download_library(
            library,
            &classifier_key,
            libraries_dir,
            cache_dir,
            source_launcher,
        )
        .await?;

        let excludes = library
            .get("extract")
            .and_then(|e| e.get("exclude"))
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| vec!["META-INF/".to_string()]);

        return extract_zip_excluding(&jar_path, natives_dir, excludes).await;
    }

    if let Some(artifact) = library
        .get("downloads")
        .and_then(|d| d.get("artifact"))
        .and_then(Value::as_object)
    {
        let name = library
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let os_alias = if current_os == "macos" {
            "osx"
        } else {
            current_os
        };
        let is_native = name.contains("natives")
            || name.contains(&format!("natives-{current_os}"))
            || name.contains(&format!("natives-{os_alias}"));

        if is_native {
            if let Some(relative_path) = artifact.get("path").and_then(Value::as_str) {
                log::info!("[REDIRECT] Buscando JAR nativo: {}", relative_path);
                let jar_path = find_or_download_artifact(
                    relative_path,
                    artifact.get("url").and_then(Value::as_str),
                    artifact.get("sha1").and_then(Value::as_str),
                    libraries_dir,
                    cache_dir,
                    source_launcher,
                )
                .await?;
                return extract_zip_excluding(
                    &jar_path,
                    natives_dir,
                    vec!["META-INF/".to_string()],
                )
                .await;
            }
        }
    }

    Ok(0)
}

fn is_modern_natives_format(version_json: &Value) -> bool {
    version_json
        .get("libraries")
        .and_then(Value::as_array)
        .map(|libraries| libraries.iter().all(|lib| lib.get("natives").is_none()))
        .unwrap_or(false)
}

async fn download_natives_from_mojang_manifest(
    version_id: &str,
    cache_dir: &Path,
    natives_dir: &Path,
    current_os: &str,
    current_arch: &str,
) -> Result<usize, String> {
    log::info!("[REDIRECT] Descargando manifest de Mojang...");
    let client = build_async_official_client()?;
    let manifest: Value = client
        .get(MOJANG_MANIFEST_URL)
        .send()
        .await
        .and_then(|res| res.error_for_status())
        .map_err(|e| format!("No se pudo descargar manifest de Mojang: {e}"))?
        .json()
        .await
        .map_err(|e| format!("No se pudo parsear manifest de Mojang: {e}"))?;

    let version_entry = manifest
        .get("versions")
        .and_then(Value::as_array)
        .and_then(|entries| {
            entries
                .iter()
                .find(|v| v.get("id").and_then(Value::as_str) == Some(version_id))
        })
        .ok_or_else(|| format!("Versión {version_id} no encontrada en manifest de Mojang"))?;
    let version_url = version_entry
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| "URL de versión no encontrada en manifest".to_string())?;

    log::info!(
        "[REDIRECT] Descargando version.json oficial para {}...",
        version_id
    );
    let version_json: Value = client
        .get(version_url)
        .send()
        .await
        .and_then(|res| res.error_for_status())
        .map_err(|e| format!("No se pudo descargar version.json oficial: {e}"))?
        .json()
        .await
        .map_err(|e| format!("No se pudo parsear version.json oficial: {e}"))?;

    let version_json_cache = cache_dir
        .join("versions")
        .join(version_id)
        .join(format!("{version_id}.json"));
    if let Some(parent) = version_json_cache.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let _ = tokio::fs::write(&version_json_cache, version_json.to_string()).await;

    let libraries = version_json
        .get("libraries")
        .and_then(Value::as_array)
        .ok_or_else(|| "version.json oficial no tiene libraries".to_string())?;
    let mut extracted = 0usize;

    for library in libraries {
        if !library_rules_allow(library, current_os, current_arch) {
            continue;
        }
        match extract_single_native(
            library,
            &cache_dir.join("libraries"),
            cache_dir,
            natives_dir,
            current_os,
            current_arch,
            "Auto detectado",
        )
        .await
        {
            Ok(count) => extracted = extracted.saturating_add(count),
            Err(err) => log::warn!("[REDIRECT] Native fallido desde manifest oficial: {err}"),
        }
    }

    Ok(extracted)
}

pub async fn prepare_redirect_natives(
    _app: &AppHandle,
    version_json: &Value,
    version_id: &str,
    libraries_dir: &Path,
    redirect_cache_dir: &Path,
    natives_dir: &Path,
    source_launcher: &str,
) -> Result<(), String> {
    tokio::fs::create_dir_all(natives_dir).await.map_err(|e| {
        format!(
            "No se pudo crear natives_dir {}: {e}",
            natives_dir.display()
        )
    })?;

    let current_os = current_os_name();
    let current_arch = current_arch_name();
    log::info!("[REDIRECT] OS: {}, Arch: {}", current_os, current_arch);

    let libraries = version_json
        .get("libraries")
        .and_then(Value::as_array)
        .ok_or_else(|| "version.json no tiene campo 'libraries'".to_string())?;

    log::info!(
        "[REDIRECT] Libraries totales en version.json: {}",
        libraries.len()
    );
    let native_libs = libraries
        .iter()
        .filter(|lib| lib.get("natives").is_some())
        .collect::<Vec<_>>();
    log::info!(
        "[REDIRECT] Libraries nativas identificadas: {}",
        native_libs.len()
    );
    for lib in &native_libs {
        if let Some(name) = lib.get("name").and_then(Value::as_str) {
            log::info!("[REDIRECT]   native: {}", name);
        }
    }

    let mut extracted = 0usize;
    let mut failed = Vec::new();

    for library in libraries {
        if !library_rules_allow(library, current_os, current_arch) {
            continue;
        }

        match extract_single_native(
            library,
            libraries_dir,
            redirect_cache_dir,
            natives_dir,
            current_os,
            current_arch,
            source_launcher,
        )
        .await
        {
            Ok(count) => extracted = extracted.saturating_add(count),
            Err(err) => {
                log::warn!("[REDIRECT] Native fallido (no fatal): {err}");
                failed.push(err);
            }
        }
    }

    log::info!("[REDIRECT] Natives extraídos: {} archivos", extracted);

    if extracted == 0 {
        log::warn!("[REDIRECT] Cero natives extraídos, intentando desde manifest de Mojang...");
        extracted = download_natives_from_mojang_manifest(
            version_id,
            redirect_cache_dir,
            natives_dir,
            current_os,
            current_arch,
        )
        .await?;
    }

    if extracted == 0 {
        if is_modern_natives_format(version_json) {
            log::info!(
                "[REDIRECT] Versión moderna detectada — natives incluidos en JARs, no requiere extracción separada"
            );
            return Ok(());
        }
        return Err(format!(
            "No se pudieron preparar los natives para {version_id} en {source_launcher}. Fallos: {}",
            failed.join(", ")
        ));
    }

    Ok(())
}

fn has_any_file(root: &Path) -> bool {
    let Ok(entries) = fs::read_dir(root) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        path.is_file() || (path.is_dir() && has_any_file(&path))
    })
}

fn emit_redirect_cache_status(app: &AppHandle, payload: serde_json::Value) {
    let _ = app.emit("redirect_cache_status", payload);
}

fn link_or_copy(existing: &Path, target: &Path) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "No se pudo crear carpeta de destino {}: {err}",
                parent.display()
            )
        })?;
    }

    #[cfg(unix)]
    {
        if std::os::unix::fs::symlink(existing, target).is_ok() {
            return Ok(());
        }
    }

    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_file(existing, target).is_ok() {
            return Ok(());
        }
    }

    fs::copy(existing, target).map(|_| ()).map_err(|err| {
        format!(
            "No se pudo copiar {} -> {}: {err}",
            existing.display(),
            target.display()
        )
    })
}

fn build_async_official_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(official_timeout())
        .connect_timeout(std::time::Duration::from_secs(30))
        .tcp_keepalive(std::time::Duration::from_secs(60))
        .user_agent("InterfaceLauncher/0.1")
        .build()
        .map_err(|err| format!("No se pudo construir cliente HTTP oficial de Minecraft: {err}"))
}

async fn download_async_with_retry(
    client: &reqwest::Client,
    url: &str,
    target_path: &Path,
    expected_sha1: &str,
    force: bool,
) -> Result<bool, String> {
    ensure_official_binary_url(url)?;

    if target_path.exists() && !force {
        if expected_sha1.is_empty() {
            return Ok(false);
        }

        let current_sha1 = crate::infrastructure::checksum::sha1::compute_file_sha1(target_path)?;
        if current_sha1.eq_ignore_ascii_case(expected_sha1) {
            return Ok(false);
        }

        let _ = fs::remove_file(target_path);
    }

    if let Some(parent) = target_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|err| {
            format!(
                "No se pudo crear directorio para descarga {}: {err}",
                parent.display()
            )
        })?;
    }

    let max_attempts = official_retries();
    let mut last_error = String::new();
    for attempt in 1..=max_attempts {
        let result: Result<(), String> = async {
            let response = client
                .get(url)
                .send()
                .await
                .map_err(|err| explain_network_error(url, &err))?;
            let status = response.status();
            if !status.is_success() {
                return Err(format!("HTTP {} al descargar {}", status.as_u16(), url));
            }

            let temp_path = target_path.with_extension("tmp");
            let mut file = tokio::fs::File::create(&temp_path).await.map_err(|err| {
                format!(
                    "No se pudo crear archivo temporal {}: {err}",
                    temp_path.display()
                )
            })?;

            let mut stream = response.bytes_stream();
            use sha1::Digest;
            let mut hasher = sha1::Sha1::new();
            use futures_util::StreamExt;
            while let Some(chunk) = stream.next().await {
                let chunk = chunk
                    .map_err(|err| format!("No se pudo leer respuesta HTTP de {url}: {err}"))?;
                file.write_all(&chunk).await.map_err(|err| {
                    format!(
                        "No se pudo escribir archivo temporal {}: {err}",
                        temp_path.display()
                    )
                })?;

                if !expected_sha1.is_empty() {
                    hasher.update(&chunk);
                }
            }

            file.flush().await.map_err(|err| {
                format!(
                    "No se pudo hacer flush del archivo temporal {}: {err}",
                    temp_path.display()
                )
            })?;
            drop(file);

            let downloaded_sha1 = if expected_sha1.is_empty() {
                String::new()
            } else {
                format!("{:x}", hasher.finalize())
            };

            if !expected_sha1.is_empty() && !downloaded_sha1.eq_ignore_ascii_case(expected_sha1) {
                let _ = tokio::fs::remove_file(&temp_path).await;
                return Err(format!(
                    "SHA1 inválido para {}. Esperado {}, obtenido {}",
                    url, expected_sha1, downloaded_sha1
                ));
            }

            tokio::fs::rename(&temp_path, target_path)
                .await
                .map_err(|err| {
                    format!(
                        "No se pudo mover {} a {}: {err}",
                        temp_path.display(),
                        target_path.display()
                    )
                })?;

            if !expected_sha1.is_empty() {
                let disk_sha1 =
                    crate::infrastructure::checksum::sha1::compute_file_sha1(target_path)?;
                if !disk_sha1.eq_ignore_ascii_case(expected_sha1) {
                    let _ = tokio::fs::remove_file(target_path).await;
                    return Err(format!(
                        "SHA1 inválido tras escritura para {}. Esperado {}, obtenido {}",
                        target_path.display(),
                        expected_sha1,
                        disk_sha1
                    ));
                }
            }

            Ok(())
        }
        .await;

        match result {
            Ok(()) => return Ok(true),
            Err(err) => {
                last_error = err;
                let temp_path = target_path.with_extension("tmp");
                if tokio::fs::metadata(&temp_path).await.is_ok() {
                    let _ = tokio::fs::remove_file(&temp_path).await;
                }

                if attempt < max_attempts {
                    let wait_secs = 2u64.pow(attempt as u32);
                    sleep(std::time::Duration::from_secs(wait_secs)).await;
                }
            }
        }
    }

    Err(format!(
        "Fallo al descargar recurso oficial tras {} intentos: {}",
        max_attempts, last_error
    ))
}

async fn load_manifest_version_url(
    client: &reqwest::Client,
    version_id: &str,
) -> Result<String, String> {
    let manifest: Value = client
        .get(MOJANG_MANIFEST_URL)
        .send()
        .await
        .and_then(|res| res.error_for_status())
        .map_err(|err| format!("No se pudo descargar manifest oficial: {err}"))?
        .json()
        .await
        .map_err(|err| format!("No se pudo parsear manifest oficial: {err}"))?;

    if let Some(url) = manifest
        .get("versions")
        .and_then(Value::as_array)
        .and_then(|versions| {
            versions
                .iter()
                .find(|entry| entry.get("id").and_then(Value::as_str) == Some(version_id))
                .and_then(|entry| entry.get("url").and_then(Value::as_str))
        })
        .map(ToOwned::to_owned)
    {
        return Ok(url);
    }

    let fallback_id = version_id
        .split('-')
        .next()
        .map(str::trim)
        .filter(|id| !id.is_empty() && *id != version_id);

    if let Some(fallback_id) = fallback_id {
        if let Some(url) = manifest
            .get("versions")
            .and_then(Value::as_array)
            .and_then(|versions| {
                versions
                    .iter()
                    .find(|entry| entry.get("id").and_then(Value::as_str) == Some(fallback_id))
                    .and_then(|entry| entry.get("url").and_then(Value::as_str))
            })
            .map(ToOwned::to_owned)
        {
            log::warn!(
                "[REDIRECT] {} no existe en manifest oficial; usando fallback vanilla {}.",
                version_id,
                fallback_id
            );
            return Ok(url);
        }
    }

    Err(format!(
        "No se encontró la versión {version_id} en manifest oficial."
    ))
}

async fn fetch_version_json_from_manifest(
    client: &reqwest::Client,
    version_id: &str,
) -> Result<Value, String> {
    let version_url = load_manifest_version_url(client, version_id).await?;
    let raw = client
        .get(&version_url)
        .send()
        .await
        .and_then(|res| res.error_for_status())
        .map_err(|err| format!("No se pudo descargar version json {version_url}: {err}"))?
        .bytes()
        .await
        .map_err(|err| format!("No se pudo leer version json: {err}"))?;

    serde_json::from_slice(&raw)
        .map_err(|err| format!("No se pudo parsear version json oficial para {version_id}: {err}"))
}

async fn download_redirect_runtime(
    app: &AppHandle,
    source_path: &Path,
    instance_uuid: &str,
    version_id: &str,
    source_launcher: &str,
) -> Result<RedirectCacheEntry, String> {
    let cache_root = redirect_cache_root(app)?;
    let entry_dir = entry_cache_dir(&cache_root, instance_uuid);
    let versions_dir = entry_dir.join("versions").join(version_id);
    let libs_dir = entry_dir.join("libraries");
    let assets_indexes_dir = entry_dir.join("assets").join("indexes");
    fs::create_dir_all(&versions_dir)
        .map_err(|err| format!("No se pudo crear versions cache: {err}"))?;
    fs::create_dir_all(&libs_dir)
        .map_err(|err| format!("No se pudo crear libraries cache: {err}"))?;
    fs::create_dir_all(&assets_indexes_dir)
        .map_err(|err| format!("No se pudo crear assets/indexes cache: {err}"))?;

    emit_redirect_cache_status(
        app,
        json!({
            "stage": "downloading",
            "instance_uuid": instance_uuid.clone(),
            "version_id": version_id,
            "message": format!("Descargando metadata de versión {version_id}..."),
            "progress_percent": 10,
            "downloaded_bytes": 0,
            "total_bytes": 0,
            "current_file": format!("{version_id}.json")
        }),
    );

    let client = build_async_official_client()?;
    let version_json_path = versions_dir.join(format!("{version_id}.json"));
    let version_json = match resolve_official_version_json(version_id, source_path, source_launcher)
    {
        Ok((local_path, json)) => {
            tokio::fs::copy(&local_path, &version_json_path)
                .await
                .map_err(|err| {
                    format!(
                        "No se pudo copiar version json local {} a caché ({}): {err}",
                        local_path.display(),
                        version_json_path.display()
                    )
                })?;
            json
        }
        Err(_) => {
            let json = fetch_version_json_from_manifest(&client, version_id).await?;
            let raw = serde_json::to_vec_pretty(&json)
                .map_err(|err| format!("No se pudo serializar version json oficial: {err}"))?;
            tokio::fs::write(&version_json_path, &raw)
                .await
                .map_err(|err| {
                    format!("No se pudo guardar {}: {err}", version_json_path.display())
                })?;
            json
        }
    };

    let parent_json = if let Some(parent_id) = version_json
        .get("inheritsFrom")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        let parent = match resolve_official_version_json(parent_id, source_path, source_launcher) {
            Ok((_, json)) => json,
            Err(_) => fetch_version_json_from_manifest(&client, parent_id).await?,
        };
        Some(parent)
    } else {
        None
    };

    emit_redirect_cache_status(
        app,
        json!({
            "stage": "downloading",
            "instance_uuid": instance_uuid.clone(),
            "version_id": version_id,
            "message": format!("Descargando client jar {version_id}..."),
            "progress_percent": 30,
            "downloaded_bytes": 0,
            "total_bytes": 0,
            "current_file": format!("{version_id}.jar")
        }),
    );

    let jar_path = versions_dir.join(format!("{version_id}.jar"));
    let jar_source_json = if version_json
        .get("downloads")
        .and_then(|v| v.get("client"))
        .is_some()
    {
        &version_json
    } else {
        parent_json.as_ref().ok_or_else(|| {
            "version json no contiene downloads.client ni se pudo resolver inheritsFrom".to_string()
        })?
    };

    if let Some(downloads) = jar_source_json
        .get("downloads")
        .and_then(|v| v.get("client"))
    {
        let url = downloads
            .get("url")
            .and_then(Value::as_str)
            .ok_or_else(|| "downloads.client.url faltante".to_string())?;
        let sha1 = downloads
            .get("sha1")
            .and_then(Value::as_str)
            .unwrap_or_default();
        download_async_with_retry(&client, url, &jar_path, sha1, false).await?;
    } else {
        return Err("version json no contiene downloads.client".to_string());
    }

    emit_redirect_cache_status(
        app,
        json!({
            "stage": "downloading",
            "instance_uuid": instance_uuid.clone(),
            "version_id": version_id,
            "message": format!("Sincronizando libraries para {version_id}..."),
            "progress_percent": 60,
            "downloaded_bytes": 0,
            "total_bytes": 0,
            "current_file": "libraries"
        }),
    );

    let rule_context = RuleContext::current();
    let system_libs = system_minecraft_root().map(|p| p.join("libraries"));
    let mut libraries_to_sync = Vec::new();
    if let Some(parent) = &parent_json {
        libraries_to_sync.extend(
            parent
                .get("libraries")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        );
    }
    libraries_to_sync.extend(
        version_json
            .get("libraries")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
    );

    for lib in libraries_to_sync {
        if let Some(rules) = lib.get("rules").and_then(Value::as_array) {
            if !evaluate_rules(rules, &rule_context) {
                continue;
            }
        }

        let artifact = lib.get("downloads").and_then(|d| d.get("artifact"));
        let rel = artifact
            .and_then(|a| a.get("path"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        if rel.is_empty() {
            continue;
        }

        let target = libs_dir.join(rel);
        if target.exists() {
            continue;
        }

        if let Some(system_root) = &system_libs {
            let global_file = system_root.join(rel);
            if global_file.exists() {
                let _ = link_or_copy(&global_file, &target);
                if target.exists() {
                    continue;
                }
            }
        }

        let sha1 = artifact
            .and_then(|a| a.get("sha1"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let url = artifact
            .and_then(|a| a.get("url"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("https://libraries.minecraft.net/{rel}"));
        download_async_with_retry(&client, &url, &target, sha1, false).await?;
    }

    emit_redirect_cache_status(
        app,
        json!({
            "stage": "downloading",
            "instance_uuid": instance_uuid.clone(),
            "version_id": version_id,
            "message": format!("Descargando índice de assets de {version_id}..."),
            "progress_percent": 85,
            "downloaded_bytes": 0,
            "total_bytes": 0,
            "current_file": "assets-index"
        }),
    );

    if let Some(asset_index) = version_json
        .get("assetIndex")
        .or_else(|| parent_json.as_ref().and_then(|json| json.get("assetIndex")))
    {
        let id = asset_index
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("legacy");
        let url = asset_index
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !url.is_empty() {
            let bytes = client
                .get(url)
                .send()
                .await
                .and_then(|res| res.error_for_status())
                .map_err(|err| format!("No se pudo descargar assets index {url}: {err}"))?
                .bytes()
                .await
                .map_err(|err| format!("No se pudo leer assets index: {err}"))?;
            let asset_path = assets_indexes_dir.join(format!("{id}.json"));
            tokio::fs::write(&asset_path, &bytes)
                .await
                .map_err(|err| format!("No se pudo guardar {}: {err}", asset_path.display()))?;

            emit_redirect_cache_status(
                app,
                json!({
                    "stage": "downloading",
                    "instance_uuid": instance_uuid.clone(),
                    "version_id": version_id,
                    "message": format!("Descargando objetos de assets para {id}..."),
                    "progress_percent": 92,
                    "downloaded_bytes": 0,
                    "total_bytes": 0,
                    "current_file": format!("assets/{id}")
                }),
            );

            let parsed_index: Value = serde_json::from_slice(&bytes)
                .map_err(|err| format!("No se pudo parsear assets index {id}: {err}"))?;
            download_assets_objects_for_redirect(
                &client,
                &parsed_index,
                source_path,
                source_launcher,
                &entry_dir.join("assets"),
            )
            .await?;
        }
    }

    let created_at = now_rfc3339();
    Ok(RedirectCacheEntry {
        instance_uuid: instance_uuid.to_string(),
        version_id: version_id.to_string(),
        source_path: source_path.display().to_string(),
        source_launcher: source_launcher.to_string(),
        created_at: created_at.clone(),
        last_used_at: created_at,
        expires_after_days: DEFAULT_CACHE_EXPIRY_DAYS,
        size_bytes: folder_size_bytes(&entry_dir),
        complete: true,
        version_json_cached: version_json_path.exists(),
        jar_cached: jar_path.exists(),
        libraries_cached: libs_dir.exists(),
        assets_cached: assets_indexes_dir.exists(),
    })
}

async fn download_assets_objects_for_redirect(
    client: &reqwest::Client,
    assets_index: &Value,
    source_path: &Path,
    source_launcher: &str,
    cache_assets_dir: &Path,
) -> Result<(), String> {
    let objects = assets_index
        .get("objects")
        .and_then(Value::as_object)
        .ok_or_else(|| "assets index no contiene objects".to_string())?;

    let object_roots = {
        let mut roots = vec![
            source_path.join("assets").join("objects"),
            source_path.join("minecraft").join("assets").join("objects"),
            source_path
                .join(".minecraft")
                .join("assets")
                .join("objects"),
        ];
        if let Some(system_root) = system_minecraft_root() {
            roots.push(system_root.join("assets").join("objects"));
        }
        roots.extend(
            launcher_roots_for_source(source_launcher)
                .into_iter()
                .map(|root| root.join("assets").join("objects")),
        );
        unique_paths(roots)
    };

    for obj in objects.values() {
        let Some(hash) = obj.get("hash").and_then(Value::as_str) else {
            continue;
        };
        if hash.len() < 2 {
            continue;
        }

        let prefix = &hash[..2];
        let target = cache_assets_dir.join("objects").join(prefix).join(hash);
        if target.exists() {
            continue;
        }

        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|err| format!("No se pudo crear directorio de assets: {err}"))?;
        }

        let mut linked = false;
        for root in &object_roots {
            let candidate = root.join(prefix).join(hash);
            if candidate.is_file() && link_or_copy(&candidate, &target).is_ok() && target.exists() {
                linked = true;
                break;
            }
        }

        if linked {
            continue;
        }

        let url = format!("https://resources.download.minecraft.net/{prefix}/{hash}");
        download_async_with_retry(client, &url, &target, hash, false).await?;
    }

    Ok(())
}

fn resolve_from_cache(
    app: &AppHandle,
    instance_uuid: &str,
    version_id: &str,
    source_path: &Path,
) -> Result<RedirectLaunchContext, String> {
    let cache_root = redirect_cache_root(app)?;
    let base = entry_cache_dir(&cache_root, instance_uuid);
    let version_json_path = base
        .join("versions")
        .join(version_id)
        .join(format!("{version_id}.json"));
    let minecraft_jar = base
        .join("versions")
        .join(version_id)
        .join(format!("{version_id}.jar"));
    if !version_json_path.exists() || !minecraft_jar.exists() {
        return Err("Entrada redirect-cache incompleta.".to_string());
    }

    let version_json: Value = serde_json::from_str(
        &fs::read_to_string(&version_json_path)
            .map_err(|err| format!("No se pudo leer {}: {err}", version_json_path.display()))?,
    )
    .map_err(|err| format!("No se pudo parsear {}: {err}", version_json_path.display()))?;

    let game_dir = resolve_redirect_game_dir(source_path);
    let assets_dir = base.join("assets");

    Ok(RedirectLaunchContext {
        version_json_path,
        version_json,
        game_dir,
        versions_dir: base.join("versions"),
        libraries_dir: base.join("libraries"),
        assets_dir,
        minecraft_jar,
        launcher_name: "REDIRECT_CACHE".to_string(),
    })
}

async fn ensure_redirect_cache_context(
    app: &AppHandle,
    source_path: &Path,
    source_launcher: &str,
    instance_uuid: &str,
    version_id: &str,
) -> Result<RedirectLaunchContext, String> {
    let cache_root = redirect_cache_root(app)?;
    let mut index = load_redirect_cache_index(&cache_root);

    if let Some(entry) = index.entries.iter().find(|entry| {
        entry.instance_uuid == instance_uuid
            && entry.version_id == version_id
            && entry.complete
            && !entry_expired(entry)
    }) {
        let _ = app.emit(
            "redirect_cache_status",
            json!({
                "stage":"ready",
                "instance_uuid": instance_uuid.clone(),
                "version_id": version_id,
                "message":"Usando caché temporal REDIRECT existente.",
                "progress_percent":100,
                "downloaded_bytes":entry.size_bytes,
                "total_bytes":entry.size_bytes,
                "current_file":"cached"
            }),
        );
        return resolve_from_cache(app, instance_uuid, version_id, source_path);
    }

    remove_cache_entry(&cache_root, &mut index, instance_uuid);
    index.entries.push(RedirectCacheEntry {
        instance_uuid: instance_uuid.to_string(),
        version_id: version_id.to_string(),
        source_path: source_path.display().to_string(),
        source_launcher: source_launcher.to_string(),
        created_at: now_rfc3339(),
        last_used_at: now_rfc3339(),
        expires_after_days: DEFAULT_CACHE_EXPIRY_DAYS,
        size_bytes: 0,
        complete: false,
        version_json_cached: false,
        jar_cached: false,
        libraries_cached: false,
        assets_cached: false,
    });
    save_redirect_cache_index(&cache_root, &index)?;

    let downloaded =
        download_redirect_runtime(app, source_path, instance_uuid, version_id, source_launcher)
            .await?;

    index
        .entries
        .retain(|entry| entry.instance_uuid != instance_uuid);
    index.entries.push(downloaded.clone());
    recalc_cache_totals(&mut index);
    save_redirect_cache_index(&cache_root, &index)?;

    emit_redirect_cache_status(
        app,
        json!({
            "stage": "ready",
            "instance_uuid": instance_uuid.clone(),
            "version_id": version_id,
            "message": format!("Caché temporal REDIRECT listo para {version_id}."),
            "progress_percent": 100,
            "downloaded_bytes": downloaded.size_bytes,
            "total_bytes": downloaded.size_bytes,
            "current_file": "done"
        }),
    );

    resolve_from_cache(app, instance_uuid, version_id, source_path)
}

pub async fn prewarm_redirect_runtime(
    app: &AppHandle,
    source_path: &Path,
    source_launcher: &str,
    instance_uuid: &str,
    version_id: &str,
) -> Result<(), String> {
    let _ =
        ensure_redirect_cache_context(app, source_path, source_launcher, instance_uuid, version_id)
            .await?;
    touch_cache_entry_last_used(app, instance_uuid);
    Ok(())
}

fn touch_cache_entry_last_used(app: &AppHandle, instance_uuid: &str) {
    if let Ok(cache_root) = redirect_cache_root(app) {
        let mut index = load_redirect_cache_index(&cache_root);
        if let Some(entry) = index
            .entries
            .iter_mut()
            .find(|entry| entry.instance_uuid == instance_uuid)
        {
            entry.last_used_at = now_rfc3339();
        }
        recalc_cache_totals(&mut index);
        let _ = save_redirect_cache_index(&cache_root, &index);
    }
}

#[tauri::command]
pub fn force_cleanup_redirect_cache(app: AppHandle) -> Result<CacheCleanupResult, String> {
    let cache_root = redirect_cache_root(&app)?;
    let mut index = load_redirect_cache_index(&cache_root);
    let result = run_redirect_cache_cleanup(&cache_root, &mut index);
    save_redirect_cache_index(&cache_root, &index)?;
    Ok(result)
}

#[tauri::command]
pub fn get_redirect_cache_info(app: AppHandle) -> Result<RedirectCacheInfo, String> {
    let cache_root = redirect_cache_root(&app)?;
    let mut index = load_redirect_cache_index(&cache_root);
    recalc_cache_totals(&mut index);
    let now = chrono::Utc::now();
    let entries = index
        .entries
        .iter()
        .map(|entry| {
            let expires_in_days = parse_rfc3339(&entry.last_used_at)
                .map(|last| entry.expires_after_days as i64 - (now - last).num_days())
                .unwrap_or(-1);
            RedirectCacheEntryInfo {
                instance_uuid: entry.instance_uuid.clone(),
                version_id: entry.version_id.clone(),
                source_launcher: entry.source_launcher.clone(),
                last_used_at: entry.last_used_at.clone(),
                expires_in_days,
                size_mb: entry.size_bytes / (1024 * 1024),
                complete: entry.complete,
            }
        })
        .collect::<Vec<_>>();

    Ok(RedirectCacheInfo {
        entries,
        total_size_bytes: index.total_size_bytes,
        total_size_mb: index.total_size_bytes / (1024 * 1024),
        max_size_mb: MAX_CACHE_SIZE_MB,
        entry_count: index.entries.len(),
        max_entries: MAX_CACHE_ENTRIES,
    })
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

pub async fn launch_redirect_instance(
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
    emit_redirect_cache_status(
        &app,
        json!({
            "stage": "checking",
            "instance_uuid": metadata.internal_uuid,
            "version_id": metadata.version_id,
            "message": "Verificando caché REDIRECT...",
            "progress_percent": 0,
            "downloaded_bytes": 0,
            "total_bytes": 0,
            "current_file": "checking"
        }),
    );

    let ctx = match resolve_redirect_launch_context(
        &source_path,
        &metadata.version_id,
        &redirect.source_launcher,
    ) {
        Ok(ctx) => ctx,
        Err(_) => {
            ensure_redirect_cache_context(
                &app,
                &source_path,
                &redirect.source_launcher,
                &metadata.internal_uuid,
                &metadata.version_id,
            )
            .await?
        }
    };
    touch_cache_entry_last_used(&app, &metadata.internal_uuid);
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
    let redirect_cache_dir = redirect_cache_root(&app)?.join(&metadata.internal_uuid);
    prepare_redirect_natives(
        &app,
        &ctx.version_json,
        &metadata.version_id,
        &ctx.libraries_dir,
        &redirect_cache_dir,
        &natives_dir,
        &redirect.source_launcher,
    )
    .await?;
    if !has_any_file(&natives_dir) {
        return Err(format!(
            "No se encontraron ni pudieron descargarse los natives para {} en {}. Verifica que la versión esté instalada en el launcher de origen.",
            metadata.version_id, redirect.source_launcher
        ));
    }

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

    for warning in verify_game_dir_has_instance_data(&ctx.game_dir) {
        log::warn!("[REDIRECT] Advertencia game_dir: {warning}");
    }

    log::info!("=== REDIRECT LAUNCH DIAGNOSTICS ===");
    log::info!("version_id:      {}", metadata.version_id);
    log::info!("source_launcher: {}", redirect.source_launcher);
    log::info!("source_path:     {}", source_path.display());
    log::info!("game_dir:        {}", ctx.game_dir.display());
    log::info!("version_json:    {}", ctx.version_json_path.display());
    log::info!("libraries_dir:   {}", ctx.libraries_dir.display());
    log::info!("assets_dir:      {}", ctx.assets_dir.display());
    log::info!("natives_dir:     {}", natives_dir.display());
    log::info!("java_path:       {}", java_exec.display());
    log::info!("ram_mb:          {}", metadata.ram_mb.max(512));
    log::info!("game_dir exists: {}", ctx.game_dir.exists());
    log::info!("mods found:      {}", ctx.game_dir.join("mods").exists());
    log::info!(
        "options found:   {}",
        ctx.game_dir.join("options.txt").exists()
    );
    log::info!(
        "shaders found:   {}",
        ctx.game_dir.join("shaderpacks").exists()
    );
    log::info!("===================================");

    let mut command = Command::new(&java_exec);
    command
        .args(&jvm_args)
        .arg(resolved.main_class.clone())
        .args(&resolved.game)
        .env(
            "JAVA_HOME",
            java_exec
                .parent()
                .and_then(Path::parent)
                .unwrap_or_else(|| Path::new(""))
                .display()
                .to_string(),
        )
        .env("APPDATA", std::env::var("APPDATA").unwrap_or_default())
        .env("HOME", std::env::var("HOME").unwrap_or_default())
        .env(
            "XDG_DATA_HOME",
            std::env::var("XDG_DATA_HOME").unwrap_or_default(),
        )
        .env_remove("_JAVA_OPTIONS")
        .env_remove("JAVA_TOOL_OPTIONS")
        .env_remove("JDK_JAVA_OPTIONS")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .current_dir(&ctx.game_dir);

    #[cfg(debug_assertions)]
    {
        log::debug!("[REDIRECT] Comando de lanzamiento:");
        log::debug!("  Java: {}", java_exec.display());
        log::debug!("  working_dir: {}", ctx.game_dir.display());
        log::debug!("  natives_dir: {}", natives_dir.display());
        for arg in jvm_args
            .iter()
            .chain(std::iter::once(&resolved.main_class))
            .chain(resolved.game.iter())
        {
            log::debug!("  arg: {arg}");
        }
    }

    #[cfg(unix)]
    {
        command.process_group(0);
    }

    let mut child = command.spawn().map_err(|err| {
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
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
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
    let instance_root_for_thread = instance_root.clone();
    let registry_instance_root = instance_root.clone();
    thread::spawn(move || {
        let mut stream_threads = Vec::new();

        if let Some(stdout_pipe) = stdout {
            let app_for_stdout = app_for_thread.clone();
            let instance_for_stdout = instance_root_for_thread.clone();
            stream_threads.push(thread::spawn(move || {
                let reader = BufReader::new(stdout_pipe);
                for line in reader.lines().map_while(Result::ok) {
                    if line.trim().is_empty() {
                        continue;
                    }
                    let _ = app_for_stdout.emit(
                        "instance_runtime_output",
                        RuntimeOutputEvent {
                            instance_root: instance_for_stdout.clone(),
                            stream: "stdout".to_string(),
                            line,
                        },
                    );
                }
            }));
        }

        if let Some(stderr_pipe) = stderr {
            let app_for_stderr = app_for_thread.clone();
            let instance_for_stderr = instance_root_for_thread.clone();
            stream_threads.push(thread::spawn(move || {
                let reader = BufReader::new(stderr_pipe);
                for line in reader.lines().map_while(Result::ok) {
                    if line.trim().is_empty() {
                        continue;
                    }
                    let _ = app_for_stderr.emit(
                        "instance_runtime_output",
                        RuntimeOutputEvent {
                            instance_root: instance_for_stderr.clone(),
                            stream: "stderr".to_string(),
                            line,
                        },
                    );
                }
            }));
        }

        for handle in stream_threads {
            let _ = handle.join();
        }

        let exit_code = child.wait().ok().and_then(|status| status.code());
        let _ = app_for_thread.emit(
            "redirect_launch_status",
            json!({
                "stage":"closed",
                "message":"Instancia REDIRECT finalizada.",
                "instance_uuid": instance_uuid.clone(),
                "source_launcher": source_launcher.clone(),
                "exit_code": exit_code,
                "error": Value::Null,
            }),
        );
        let _ = app_for_thread.emit(
            "instance_runtime_exit",
            serde_json::json!({
                "instanceRoot": instance_root_for_thread,
                "exitCode": exit_code,
                "pid": pid,
            }),
        );
        crate::app::instance_service::register_runtime_exit(
            &registry_instance_root,
            pid,
            exit_code,
        );
        let _ = fs::remove_dir_all(&natives_dir);
        touch_cache_entry_last_used(&app_for_thread, &instance_uuid);
        let _ = cleanup_redirect_cache_after_launch(&app_for_thread);
    });

    Ok(StartInstanceResult {
        pid,
        java_path: java_exec.display().to_string(),
        logs,
        refreshed_auth_session: auth_session,
    })
}
