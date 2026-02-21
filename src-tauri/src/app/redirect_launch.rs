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
    commands::import::resolve_effective_version_id,
    domain::{
        auth::microsoft::refresh_microsoft_access_token,
        auth::xbox::{
            authenticate_with_xbox_live, authorize_xsts, login_minecraft_with_xbox,
            read_minecraft_profile,
        },
        minecraft::{
            argument_resolver::{resolve_launch_arguments, LaunchContext},
            rule_engine::{evaluate_rules, RuleContext, RuleFeatures},
        },
        models::{
            instance::{InstanceMetadata, LaunchAuthSession},
            java::JavaRuntime,
        },
    },
    infrastructure::downloader::queue::{
        ensure_official_binary_url, explain_network_error, official_retries, official_timeout,
    },
    services::{instance_builder::build_instance_structure, java_installer::ensure_embedded_java},
};

const DEFAULT_CACHE_EXPIRY_DAYS: u32 = 7;
const MAX_CACHE_SIZE_MB: u64 = 2048;
const MAX_CACHE_ENTRIES: usize = 10;
const MOJANG_MANIFEST_URL: &str =
    "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json";

#[derive(Debug, Clone)]
pub struct RedirectLaunchContext {
    pub resolved_version_id: String,
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
pub struct RedirectVersionHints {
    pub minecraft_version: String,
    pub loader: String,
    pub loader_version: String,
}

fn normalize_loader(loader: &str) -> String {
    let lower = loader.trim().to_ascii_lowercase();
    match lower.as_str() {
        "quilit" => "quilt".to_string(),
        _ => lower,
    }
}

fn build_version_id_candidates(version_id: &str, hints: &RedirectVersionHints) -> Vec<String> {
    let mut candidates = Vec::new();
    let push_candidate = |items: &mut Vec<String>, value: String| {
        if value.trim().is_empty() {
            return;
        }
        if !items
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&value))
        {
            items.push(value);
        }
    };

    push_candidate(&mut candidates, version_id.trim().to_string());

    let mc = hints.minecraft_version.trim();
    if !mc.is_empty() {
        push_candidate(&mut candidates, mc.to_string());
    }

    let loader = normalize_loader(&hints.loader);
    let loader_version = hints.loader_version.trim();
    if !mc.is_empty() && !loader_version.is_empty() && loader_version != "-" {
        match loader.as_str() {
            "fabric" => push_candidate(
                &mut candidates,
                format!("fabric-loader-{loader_version}-{mc}"),
            ),
            "quilt" => push_candidate(
                &mut candidates,
                format!("quilt-loader-{loader_version}-{mc}"),
            ),
            "forge" => push_candidate(&mut candidates, format!("{mc}-forge-{loader_version}")),
            "neoforge" => {
                push_candidate(&mut candidates, format!("{mc}-neoforge-{loader_version}"))
            }
            _ => {}
        }
    }

    if loader != "vanilla" && !loader.is_empty() {
        push_candidate(&mut candidates, format!("{mc}-{loader}-"));
        push_candidate(&mut candidates, format!("{loader}-loader--{mc}"));
    }

    candidates
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairInstanceResult {
    pub repaired: bool,
    pub changes_made: Vec<String>,
    pub errors: Vec<String>,
    pub final_state: String,
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

fn now_unix_millis() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis() as u64)
}

fn detect_loader_from_version_id(version_id: &str) -> Option<(String, String)> {
    let normalized = version_id.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }

    let patterns = [
        ("fabric-loader-", "fabric"),
        ("quilt-loader-", "quilt"),
        ("neoforge-", "neoforge"),
        ("forge-", "forge"),
    ];

    for (token, loader_name) in patterns {
        if let Some(pos) = normalized.find(token) {
            let raw = &normalized[(pos + token.len())..];
            let version = raw.split(['+', '-', '_']).next().unwrap_or("-");
            return Some((loader_name.to_string(), version.to_string()));
        }
    }

    None
}

fn write_instance_metadata(
    instance_path: &Path,
    metadata: &InstanceMetadata,
) -> Result<(), String> {
    let metadata_path = instance_path.join(".instance.json");
    let raw = serde_json::to_string_pretty(metadata).map_err(|err| {
        format!(
            "No se pudo serializar metadata de {}: {err}",
            instance_path.display()
        )
    })?;
    fs::write(&metadata_path, raw)
        .map_err(|err| format!("No se pudo guardar {}: {err}", metadata_path.display()))
}

async fn refresh_microsoft_token_if_needed(
    auth_session: LaunchAuthSession,
) -> Result<LaunchAuthSession, String> {
    let mut needs_refresh = auth_session.minecraft_access_token.trim().is_empty();
    if let (Some(expires_at), Some(now)) = (
        auth_session.minecraft_access_token_expires_at,
        now_unix_millis(),
    ) {
        if expires_at <= now.saturating_add(60_000) {
            needs_refresh = true;
        }
    }

    if !needs_refresh {
        return Ok(auth_session);
    }

    let refresh_token = auth_session
        .microsoft_refresh_token
        .clone()
        .ok_or_else(|| {
            "No hay refresh token de Microsoft para renovar credenciales REDIRECT.".to_string()
        })?;

    let client = reqwest::Client::new();
    let ms = refresh_microsoft_access_token(&client, &refresh_token).await?;
    let xbox = authenticate_with_xbox_live(&client, &ms.access_token).await?;
    let xsts = authorize_xsts(&client, &xbox.token).await?;
    let minecraft = login_minecraft_with_xbox(&client, &xsts.uhs, &xsts.token).await?;
    let profile = read_minecraft_profile(&client, &minecraft.access_token).await?;
    let expires_at = minecraft.expires_in.and_then(|expires_in| {
        now_unix_millis().map(|now| now.saturating_add(expires_in.saturating_mul(1000)))
    });

    Ok(LaunchAuthSession {
        profile_id: profile.id,
        profile_name: profile.name,
        minecraft_access_token: minecraft.access_token,
        minecraft_access_token_expires_at: expires_at,
        microsoft_refresh_token: ms.refresh_token.or(auth_session.microsoft_refresh_token),
        premium_verified: auth_session.premium_verified,
    })
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

pub fn clear_redirect_cache_for_instance(
    app: &AppHandle,
    instance_root: &Path,
    instance_uuid: &str,
) -> Result<(), String> {
    let cache_root = redirect_cache_root(app)?;
    let mut index = load_redirect_cache_index(&cache_root);
    remove_cache_entry(&cache_root, &mut index, instance_uuid);
    let _ = save_redirect_cache_index(&cache_root, &index);

    let prefix = instance_root.display().to_string();
    if let Ok(mut ctx_cache) = redirect_ctx_cache().lock() {
        ctx_cache.retain(|key, _| !key.starts_with(&prefix) && !key.contains(instance_uuid));
    }

    Ok(())
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
            roots.push(user_profile.join("curseforge/minecraft/instances"));
            roots.push(user_profile.join("curseforge/minecraft/install"));
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
            roots.push(home.join("Library/Application Support/curseforge/minecraft/instances"));
            roots.push(home.join("Library/Application Support/curseforge/minecraft/install"));
            roots.push(home.join("Library/Application Support/curseforge/minecraft/Install"));
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            roots.push(home.join(".local/share/PrismLauncher"));
            roots.push(home.join(".local/share/ModrinthApp/.minecraft"));
            roots.push(home.join(".local/share/MultiMC"));
            roots.push(home.join(".local/share/curseforge/minecraft/instances"));
            roots.push(home.join(".local/share/curseforge/minecraft/install"));
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

    if !has_main_class && !has_libraries && !has_arguments {
        log::warn!(
            "[REDIRECT] version.json en {} no contiene campos de lanzamiento, descartando",
            path.display()
        );
        return None;
    }

    Some(json)
}

fn find_versions_matching_prefix(versions_dir: &Path, prefix: &str) -> Vec<PathBuf> {
    let prefix_lower = prefix.trim_end_matches('-').to_ascii_lowercase();
    let Ok(entries) = fs::read_dir(versions_dir) else {
        return vec![];
    };

    entries
        .flatten()
        .filter_map(|entry| {
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy().to_string();
            let lower = name.to_ascii_lowercase();
            if !lower.contains(&prefix_lower) {
                return None;
            }
            let json = versions_dir.join(&file_name).join(format!("{name}.json"));
            if json.is_file() {
                Some(json)
            } else {
                None
            }
        })
        .collect()
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
    json: &Value,
    version_ids: &[String],
    loader_hint: Option<&str>,
    mc_hint: Option<&str>,
) -> usize {
    let mut score = 0usize;
    let lower = path.to_string_lossy().to_ascii_lowercase();
    for version_id in version_ids {
        let version_lower = version_id.to_ascii_lowercase();
        if lower.contains(&version_lower) {
            score += 30;
            break;
        }
    }

    if let Some(loader) = loader_hint {
        if lower.contains(loader) {
            score += 35;
        }
    }

    if json.get("inheritsFrom").is_some() {
        score += 50;
    }

    let main_class = json
        .get("mainClass")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    if ["fabricmc", "neoforge", "bootstraplauncher", "quilt"]
        .iter()
        .any(|needle| main_class.contains(needle))
    {
        score += 40;
    }

    let version_dir_match = version_ids.iter().any(|version_id| {
        path.parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case(version_id))
    });
    if version_dir_match {
        score += 60;
    }

    if json.get("inheritsFrom").is_none()
        && ![
            "fabricmc",
            "knot",
            "bootstraplauncher",
            "quilt",
            "launchwrapper",
        ]
        .iter()
        .any(|needle| main_class.contains(needle))
    {
        score = score.saturating_sub(20);
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

fn is_loader_version_json(json: &Value, loader: &str) -> bool {
    let main_class = json.get("mainClass").and_then(Value::as_str).unwrap_or("");
    let main_lower = main_class.to_ascii_lowercase();
    let has_inherits = json.get("inheritsFrom").is_some();
    let loader_lower = loader.to_ascii_lowercase();

    has_inherits
        || match loader_lower.as_str() {
            "fabric" => main_lower.contains("fabricmc") || main_lower.contains("knot"),
            "quilt" => main_lower.contains("quiltmc") || main_lower.contains("knot"),
            "forge" => {
                main_lower.contains("launchwrapper") || main_lower.contains("bootstraplauncher")
            }
            "neoforge" => {
                main_lower.contains("bootstraplauncher") || main_lower.contains("neoforged")
            }
            _ => false,
        }
}

fn resolve_official_version_json(
    version_ids: &[String],
    hints: &RedirectVersionHints,
    source_path: &Path,
    source_launcher: &str,
) -> Result<(PathBuf, Value), String> {
    let (manifest_loader_hint, manifest_mc_hint) = detect_source_instance_hints(source_path);
    let loader_hint = manifest_loader_hint.or_else(|| {
        let normalized = normalize_loader(&hints.loader);
        if normalized.is_empty() || normalized == "vanilla" {
            None
        } else {
            Some(normalized)
        }
    });
    let mc_hint = manifest_mc_hint.or_else(|| {
        let mc = hints.minecraft_version.trim();
        if mc.is_empty() {
            None
        } else {
            Some(mc.to_string())
        }
    });

    let mut loader_candidates = Vec::new();
    let mut vanilla_candidates = Vec::new();
    for version_id in version_ids {
        if version_id.ends_with('-') {
            for versions_dir in [
                source_path.join(".minecraft/versions"),
                source_path.join("versions"),
                source_path.join("minecraft/versions"),
            ] {
                let is_loader_version = version_id.contains("fabric")
                    || version_id.contains("forge")
                    || version_id.contains("neoforge")
                    || version_id.contains("quilt");
                let matches = find_versions_matching_prefix(&versions_dir, version_id);
                if is_loader_version {
                    loader_candidates.extend(matches);
                } else {
                    vanilla_candidates.extend(matches);
                }
            }
        }

        let is_loader_version = version_id.contains("fabric")
            || version_id.contains("forge")
            || version_id.contains("neoforge")
            || version_id.contains("quilt");
        let paths = [
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

        if is_loader_version {
            loader_candidates.extend(paths);
        } else {
            vanilla_candidates.extend(paths);
        }
    }

    for launcher_root in launcher_roots_for_source(source_launcher) {
        for version_id in version_ids {
            if version_id.ends_with('-') {
                let versions_dir = launcher_root.join("versions");
                let matches = find_versions_matching_prefix(&versions_dir, version_id);
                let is_loader = version_id.contains("fabric")
                    || version_id.contains("forge")
                    || version_id.contains("neoforge")
                    || version_id.contains("quilt");
                if is_loader {
                    loader_candidates.extend(matches);
                } else {
                    vanilla_candidates.extend(matches);
                }
            }

            let path = launcher_root
                .join("versions")
                .join(version_id)
                .join(format!("{version_id}.json"));
            let is_loader = version_id.contains("fabric")
                || version_id.contains("forge")
                || version_id.contains("neoforge")
                || version_id.contains("quilt");
            if is_loader {
                loader_candidates.push(path);
            } else {
                vanilla_candidates.push(path);
            }
        }
    }

    let mut candidate_files: Vec<PathBuf> = loader_candidates
        .into_iter()
        .chain(vanilla_candidates)
        .collect();

    if let Some(system_root) = system_minecraft_root() {
        for version_id in version_ids {
            if version_id.ends_with('-') {
                candidate_files.extend(find_versions_matching_prefix(
                    &system_root.join("versions"),
                    version_id,
                ));
            }
            candidate_files.push(
                system_root
                    .join("versions")
                    .join(version_id)
                    .join(format!("{version_id}.json")),
            );
        }
    }

    let mut scored_loader = Vec::new();
    let mut scored_vanilla = Vec::new();

    for candidate in unique_paths(candidate_files) {
        if !candidate.is_file() {
            continue;
        }
        let Some(json) = read_and_validate_version_json(&candidate) else {
            continue;
        };
        let score = score_version_json_candidate(
            &candidate,
            &json,
            version_ids,
            loader_hint.as_deref(),
            mc_hint.as_deref(),
        );

        if let Some(loader_name) = loader_hint.as_deref() {
            if is_loader_version_json(&json, loader_name) {
                scored_loader.push((score, candidate, json));
                continue;
            }
        }

        scored_vanilla.push((score, candidate, json));
    }

    scored_loader.sort_by(|a, b| b.0.cmp(&a.0));
    scored_vanilla.sort_by(|a, b| b.0.cmp(&a.0));

    let mut best = scored_loader.into_iter().next();
    if best.is_none() {
        best = scored_vanilla.into_iter().next();
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
        "No se encontró version.json válido para ninguna de las versiones [{}]. Verifica que esté instalada en {source_launcher} o en el launcher oficial de Mojang.",
        version_ids.join(", ")
    ))
}

fn resolve_redirect_game_dir(source_path: &Path) -> PathBuf {
    log::info!(
        "[REDIRECT] resolve_redirect_game_dir recibió: {}",
        source_path.display()
    );
    if let Ok(entries) = fs::read_dir(source_path) {
        let contents: Vec<String> = entries
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect();
        log::info!("[REDIRECT] Contenido de source_path: {:?}", contents);
    }

    let has_game_data = |path: &Path| path.join("mods").is_dir() || path.join("saves").is_dir();

    let dot_minecraft = source_path.join(".minecraft");
    if dot_minecraft.is_dir() && has_game_data(&dot_minecraft) {
        if let Ok(entries) = fs::read_dir(&dot_minecraft) {
            let contents: Vec<String> = entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().to_string())
                .collect();
            log::info!("[REDIRECT] Contenido de .minecraft/: {:?}", contents);
        }
        log::info!(
            "[REDIRECT] game_dir: subcarpeta .minecraft: {}",
            dot_minecraft.display()
        );
        return dot_minecraft;
    }

    let minecraft = source_path.join("minecraft");
    if minecraft.is_dir() && has_game_data(&minecraft) {
        if let Ok(entries) = fs::read_dir(&minecraft) {
            let contents: Vec<String> = entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().to_string())
                .collect();
            log::info!("[REDIRECT] Contenido de minecraft/: {:?}", contents);
        }
        log::info!(
            "[REDIRECT] game_dir: subcarpeta minecraft/: {}",
            minecraft.display()
        );
        return minecraft;
    }

    if has_game_data(source_path) {
        log::info!(
            "[REDIRECT] game_dir: source_path contiene datos directamente: {}",
            source_path.display()
        );
        return source_path.to_path_buf();
    }

    let mut queue = std::collections::VecDeque::from([(source_path.to_path_buf(), 0usize)]);
    while let Some((current, depth)) = queue.pop_front() {
        if depth >= 2 {
            continue;
        }
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let candidate = entry.path();
            if !candidate.is_dir() {
                continue;
            }
            if has_game_data(&candidate) {
                log::info!(
                    "[REDIRECT] game_dir: subcarpeta detectada con datos de juego: {}",
                    candidate.display()
                );
                return candidate;
            }
            queue.push_back((candidate, depth + 1));
        }
    }

    log::warn!(
        "[REDIRECT] game_dir: sin estructura clara, usando source_path: {}",
        source_path.display()
    );
    source_path.to_path_buf()
}

fn merge_version_jsons(parent: &Value, child: &Value) -> Value {
    use serde_json::Map;

    fn extract_maven_key(lib: &Value) -> Option<String> {
        let name = lib.get("name")?.as_str()?;
        let parts: Vec<&str> = name.splitn(4, ':').collect();
        match parts.len() {
            3 => Some(format!("{}:{}", parts[0], parts[1])),
            4 => Some(format!("{}:{}:{}", parts[0], parts[1], parts[3])),
            _ => Some(name.to_string()),
        }
    }

    let mut result: Map<String, Value> = parent.as_object().cloned().unwrap_or_default();
    let child_obj = match child.as_object() {
        Some(o) => o.clone(),
        None => return Value::Object(result),
    };

    for (key, child_val) in child_obj {
        match key.as_str() {
            "inheritsFrom" => {}
            "libraries" => {
                let parent_libs = result
                    .get("libraries")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let child_libs = child_val.as_array().cloned().unwrap_or_default();
                let mut deduped = Vec::new();
                let mut seen = std::collections::HashSet::new();
                let mut fallback_idx = 0usize;

                for lib in child_libs.into_iter().chain(parent_libs.into_iter()) {
                    let key = extract_maven_key(&lib).unwrap_or_else(|| {
                        let marker = format!("__unknown_{fallback_idx}");
                        fallback_idx += 1;
                        marker
                    });
                    if seen.insert(key) {
                        deduped.push(lib);
                    }
                }
                result.insert("libraries".to_string(), Value::Array(deduped));
            }
            "arguments" => {
                let parent_args = result
                    .get("arguments")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                let Some(child_args) = child_val.as_object().cloned() else {
                    continue;
                };
                let mut merged_args = parent_args.clone();

                let parent_game = parent_args
                    .get("game")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let child_game = child_args
                    .get("game")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let mut merged_game = parent_game;
                merged_game.extend(child_game);
                merged_args.insert("game".to_string(), Value::Array(merged_game));

                let parent_jvm = parent_args
                    .get("jvm")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let child_jvm = child_args
                    .get("jvm")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let mut merged_jvm = parent_jvm;
                merged_jvm.extend(child_jvm);
                merged_args.insert("jvm".to_string(), Value::Array(merged_jvm));
                result.insert("arguments".to_string(), Value::Object(merged_args));
            }
            "assetIndex" | "assets" => {
                if !result.contains_key(&key) {
                    result.insert(key, child_val);
                }
            }
            _ => {
                result.insert(key, child_val);
            }
        }
    }

    Value::Object(result)
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
    version_ids: &[String],
    source_launcher: &str,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    for version_id in version_ids {
        candidates.extend([
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
        ]);
    }

    if let Some(system_root) = system_minecraft_root() {
        for version_id in version_ids {
            candidates.push(
                system_root
                    .join("versions")
                    .join(version_id)
                    .join(format!("{version_id}.jar")),
            );
        }
    }

    for launcher_root in launcher_roots_for_source(source_launcher) {
        for version_id in version_ids {
            candidates.push(
                launcher_root
                    .join("versions")
                    .join(version_id)
                    .join(format!("{version_id}.jar")),
            );
        }
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
    version_ids: &[String],
    source_launcher: &str,
) -> Option<(String, PathBuf)> {
    minecraft_jar_candidates(source_path, version_ids, source_launcher)
        .into_iter()
        .find(|candidate| candidate.is_file())
        .and_then(|candidate| {
            let id = candidate
                .parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                .map(str::to_string)?;
            Some((id, candidate))
        })
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
    hints: &RedirectVersionHints,
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

    let version_ids = build_version_id_candidates(version_id, hints);
    let cache_key = format!("{}::{}", source_path.display(), version_ids.join("|"));
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
        resolve_official_version_json(&version_ids, hints, source_path, source_launcher)?;
    let parent_json = if let Some(parent_id) = version_json
        .get("inheritsFrom")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        let parent_ids = build_version_id_candidates(parent_id, hints);
        match resolve_official_version_json(&parent_ids, hints, source_path, source_launcher) {
            Ok((_, json)) => Some(json),
            Err(err) => {
                log::warn!(
                    "[REDIRECT] No se pudo resolver parent {} para merge local: {}",
                    parent_id,
                    err
                );
                None
            }
        }
    } else {
        None
    };
    let final_version_json = if let Some(parent) = &parent_json {
        merge_version_jsons(parent, &version_json)
    } else {
        version_json.clone()
    };

    let minecraft_jar_candidates =
        minecraft_jar_candidates(source_path, &version_ids, source_launcher);
    let (resolved_version_id, minecraft_jar) =
        find_minecraft_jar(source_path, &version_ids, source_launcher).ok_or_else(|| {
            format!(
                "No se encontró un JAR de versión válido para [{}]. Se buscó en:
{}

Asegúrate de que la versión esté completamente instalada en el launcher de origen.",
                version_ids.join(", "),
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
        resolved_version_id,
        version_json_path: version_json_path.clone(),
        version_json: final_version_json,
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

    if declared > 0 {
        return match declared as u32 {
            0..=11 => JavaRuntime::Java8,
            12..=20 => JavaRuntime::Java17,
            _ => JavaRuntime::Java21,
        };
    }

    let mc_version = extract_minecraft_version_from_id(version_id);
    let minor = mc_version
        .split('.')
        .nth(1)
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(20);

    match minor {
        0..=16 => JavaRuntime::Java8,
        17..=20 => JavaRuntime::Java17,
        _ => JavaRuntime::Java21,
    }
}

fn find_loader_jar_in_dirs(libraries_dirs: &[PathBuf], loader: &str) -> Option<PathBuf> {
    let needle = loader.to_ascii_lowercase();
    for libraries_dir in libraries_dirs {
        let mut queue = std::collections::VecDeque::from([libraries_dir.clone()]);
        while let Some(current) = queue.pop_front() {
            let Ok(entries) = fs::read_dir(&current) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    queue.push_back(path);
                    continue;
                }
                if path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("jar"))
                    && path
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .contains(&needle)
                {
                    return Some(path);
                }
            }
        }
    }
    None
}

fn extract_minecraft_version_from_id(version_id: &str) -> String {
    if version_id.starts_with("fabric-loader-") || version_id.starts_with("quilt-loader-") {
        if let Some(mc) = version_id.rsplitn(2, '-').next() {
            if mc.contains('.')
                && mc
                    .chars()
                    .next()
                    .map(|ch| ch.is_ascii_digit())
                    .unwrap_or(false)
            {
                return mc.to_string();
            }
        }
    }

    let lower = version_id.to_ascii_lowercase();
    for sep in ["-forge-", "-neoforge-", "-optifine_", "-optifine-"] {
        if let Some(pos) = lower.find(sep) {
            return version_id[..pos].to_string();
        }
    }

    version_id.to_string()
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

pub fn build_classpath_multi(
    merged_version_json: &serde_json::Value,
    libraries_dirs: &[PathBuf],
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

    for library in merged_version_json
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
        let Some(relative) = maven_library_path(name) else {
            continue;
        };

        let found = libraries_dirs.iter().find_map(|dir| {
            let full = dir.join(&relative);
            if full.exists() {
                Some(full)
            } else {
                None
            }
        });

        match found {
            Some(path) => entries.push(path.display().to_string()),
            None => log::warn!("[REDIRECT] Library faltante en todas las rutas: {name}"),
        }
    }

    let main_jar = versions_dir
        .join(version_id)
        .join(format!("{version_id}.jar"));
    if !main_jar.exists() {
        return Err(format!(
            "No se encontró {version_id}.jar en {}",
            versions_dir.display()
        ));
    }
    entries.push(main_jar.display().to_string());

    Ok(entries
        .iter()
        .map(|e| {
            if cfg!(target_os = "windows") {
                e.replace('/', "\\")
            } else {
                e.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(sep))
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

async fn fetch_loader_version_json(
    client: &reqwest::Client,
    loader: &str,
    loader_version: &str,
    minecraft_version: &str,
) -> Result<Value, String> {
    async fn download_installer_bytes(
        client: &reqwest::Client,
        urls: &[String],
        loader: &str,
        loader_version: &str,
    ) -> Result<Vec<u8>, String> {
        let mut last_error: Option<String> = None;
        for url in urls {
            match client
                .get(url)
                .send()
                .await
                .and_then(|res| res.error_for_status())
            {
                Ok(response) => {
                    let bytes = response.bytes().await.map_err(|e| {
                        format!(
                            "No se pudo leer installer de {loader} {loader_version} desde {url}: {e}"
                        )
                    })?;
                    return Ok(bytes.to_vec());
                }
                Err(err) => {
                    last_error = Some(format!(
                        "No se pudo descargar installer de {loader} {loader_version} desde {url}: {err}"
                    ));
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            format!(
                "No se pudo descargar installer de {loader} {loader_version} desde ningún mirror"
            )
        }))
    }

    fn extract_version_json_from_installer(
        installer_bytes: &[u8],
        loader: &str,
        loader_version: &str,
    ) -> Result<Value, String> {
        let reader = std::io::Cursor::new(installer_bytes);
        let mut archive = ZipArchive::new(reader).map_err(|err| {
            format!("Installer de {loader} {loader_version} inválido (zip): {err}")
        })?;

        for candidate in ["version.json", "profile.json"] {
            if let Ok(mut file) = archive.by_name(candidate) {
                let mut raw = String::new();
                use std::io::Read;
                file.read_to_string(&mut raw).map_err(|err| {
                    format!("No se pudo leer {candidate} dentro del installer de {loader}: {err}")
                })?;
                let json: Value = serde_json::from_str(&raw).map_err(|err| {
                    format!("No se pudo parsear {candidate} del installer de {loader}: {err}")
                })?;
                return Ok(json);
            }
        }

        Err(format!(
            "El installer de {loader} {loader_version} no incluye version.json/profile.json"
        ))
    }

    let url = match loader.trim().to_ascii_lowercase().as_str() {
        "fabric" => format!(
            "https://meta.fabricmc.net/v2/versions/loader/{}/{}/profile/json",
            minecraft_version, loader_version
        ),
        "quilt" => format!(
            "https://meta.quiltmc.org/v3/versions/loader/{}/{}/profile/json",
            minecraft_version, loader_version
        ),
        "forge" => {
            let urls = vec![format!(
                "https://maven.minecraftforge.net/net/minecraftforge/forge/{minecraft_version}-{loader_version}/forge-{minecraft_version}-{loader_version}-installer.jar"
            )];
            log::info!(
                "[REDIRECT] Descargando installer de Forge para extraer version.json: {}",
                urls[0]
            );
            let installer =
                download_installer_bytes(client, &urls, "forge", loader_version).await?;
            return extract_version_json_from_installer(&installer, "forge", loader_version);
        }
        "neoforge" => {
            let urls = vec![
                format!(
                    "https://maven.neoforged.net/releases/net/neoforged/neoforge/{loader_version}/neoforge-{loader_version}-installer.jar"
                ),
                format!(
                    "https://maven.neoforged.net/releases/net/neoforged/neoforge/{minecraft_version}-{loader_version}/neoforge-{minecraft_version}-{loader_version}-installer.jar"
                ),
            ];
            log::info!(
                "[REDIRECT] Descargando installer de NeoForge para extraer version.json ({} intentos).",
                urls.len()
            );
            let installer =
                download_installer_bytes(client, &urls, "neoforge", loader_version).await?;
            return extract_version_json_from_installer(&installer, "neoforge", loader_version);
        }
        _ => {
            return Err(format!(
                "Loader '{loader}' no tiene descarga automática de metadata."
            ));
        }
    };

    log::info!("[REDIRECT] Descargando version.json del loader desde: {url}");

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("No se pudo descargar version.json del loader {loader}: {e}"))?;
    let response = response
        .error_for_status()
        .map_err(|e| format!("No se pudo descargar version.json del loader {loader}: {e}"))?;

    response
        .json()
        .await
        .map_err(|e| format!("No se pudo parsear version.json del loader {loader}: {e}"))
}

async fn download_redirect_runtime(
    app: &AppHandle,
    source_path: &Path,
    instance_uuid: &str,
    version_id: &str,
    source_launcher: &str,
    hints: &RedirectVersionHints,
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
    let version_ids = build_version_id_candidates(version_id, hints);
    let mut version_json =
        match resolve_official_version_json(&version_ids, hints, source_path, source_launcher) {
            Ok((local_path, json)) => {
                let _ = tokio::fs::copy(&local_path, &version_json_path).await;
                json
            }
            Err(_) => {
                let loader = hints.loader.trim().to_ascii_lowercase();
                if !loader.is_empty() && loader != "vanilla" && !hints.loader_version.is_empty() {
                    match fetch_loader_version_json(
                        &client,
                        &loader,
                        &hints.loader_version,
                        &hints.minecraft_version,
                    )
                    .await
                    {
                        Ok(json) => {
                            let raw = serde_json::to_vec_pretty(&json).map_err(|e| {
                                format!("No se pudo serializar version.json del loader: {e}")
                            })?;
                            tokio::fs::write(&version_json_path, &raw)
                                .await
                                .map_err(|e| {
                                    format!("No se pudo guardar version.json del loader: {e}")
                                })?;
                            json
                        }
                        Err(_) => fetch_version_json_from_manifest(&client, version_id).await?,
                    }
                } else {
                    fetch_version_json_from_manifest(&client, version_id).await?
                }
            }
        };

    let loader_hint = hints.loader.trim().to_ascii_lowercase();
    if !loader_hint.is_empty()
        && loader_hint != "vanilla"
        && !is_loader_version_json(&version_json, &loader_hint)
        && !hints.loader_version.trim().is_empty()
        && hints.loader_version.trim() != "-"
    {
        log::warn!(
            "[REDIRECT] version.json resuelto parece vanilla; intentando descargar json específico del loader {} {}",
            loader_hint,
            hints.loader_version
        );
        if let Ok(loader_json) = fetch_loader_version_json(
            &client,
            &loader_hint,
            &hints.loader_version,
            &hints.minecraft_version,
        )
        .await
        {
            version_json = loader_json;
            let raw = serde_json::to_vec_pretty(&version_json)
                .map_err(|e| format!("No se pudo serializar version.json del loader: {e}"))?;
            tokio::fs::write(&version_json_path, &raw)
                .await
                .map_err(|e| format!("No se pudo guardar version.json del loader: {e}"))?;
        }
    }

    let parent_json = if let Some(parent_id) = version_json
        .get("inheritsFrom")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        let parent_ids = build_version_id_candidates(parent_id, hints);
        let parent =
            match resolve_official_version_json(&parent_ids, hints, source_path, source_launcher) {
                Ok((_, json)) => json,
                Err(_) => fetch_version_json_from_manifest(&client, parent_id).await?,
            };
        Some(parent)
    } else {
        None
    };
    let final_version_json = if let Some(parent) = &parent_json {
        merge_version_jsons(parent, &version_json)
    } else {
        version_json.clone()
    };

    let raw_merged = serde_json::to_vec_pretty(&final_version_json)
        .map_err(|err| format!("No se pudo serializar version json mergeado: {err}"))?;
    tokio::fs::write(&version_json_path, &raw_merged)
        .await
        .map_err(|err| format!("No se pudo guardar version json mergeado: {err}"))?;

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
    let jar_source_json = if final_version_json
        .get("downloads")
        .and_then(|v| v.get("client"))
        .is_some()
    {
        &final_version_json
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
    let libraries_to_sync = final_version_json
        .get("libraries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    for lib in libraries_to_sync {
        if let Some(rules) = lib.get("rules").and_then(Value::as_array) {
            if !evaluate_rules(rules, &rule_context) {
                continue;
            }
        }

        if let Some(artifact) = lib.get("downloads").and_then(|d| d.get("artifact")) {
            let rel = artifact
                .get("path")
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
                .get("sha1")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let url = artifact
                .get("url")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("https://libraries.minecraft.net/{rel}"));
            download_async_with_retry(&client, &url, &target, sha1, false).await?;
            continue;
        }

        let Some(name) = lib.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Some(relative_path) = maven_library_path(name) else {
            continue;
        };
        let rel_str = relative_path.to_string_lossy().replace('\\', "/");
        let target = libs_dir.join(&relative_path);
        if target.exists() {
            continue;
        }

        if let Some(system_root) = &system_libs {
            let global_file = system_root.join(&relative_path);
            if global_file.exists() {
                let _ = link_or_copy(&global_file, &target);
                if target.exists() {
                    continue;
                }
            }
        }

        for launcher_root in launcher_roots_for_source(source_launcher) {
            let local = launcher_root.join("libraries").join(&relative_path);
            if local.exists() {
                let _ = link_or_copy(&local, &target);
                if target.exists() {
                    break;
                }
            }
        }
        if target.exists() {
            continue;
        }

        let base_url = lib
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or("https://libraries.minecraft.net/");
        let url = format!("{}/{}", base_url.trim_end_matches('/'), rel_str);
        let sha1 = lib
            .get("downloads")
            .and_then(|d| d.get("artifact"))
            .and_then(|a| a.get("sha1"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        if let Err(err) = download_async_with_retry(&client, &url, &target, sha1, false).await {
            log::warn!("[REDIRECT] No se pudo descargar library {name}: {err}");
        }
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
        resolved_version_id: version_id.to_string(),
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
    hints: &RedirectVersionHints,
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

    let downloaded = download_redirect_runtime(
        app,
        source_path,
        instance_uuid,
        version_id,
        source_launcher,
        hints,
    )
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
    hints: &RedirectVersionHints,
) -> Result<(), String> {
    let _ = ensure_redirect_cache_context(
        app,
        source_path,
        source_launcher,
        instance_uuid,
        version_id,
        hints,
    )
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

    let hints = RedirectVersionHints {
        minecraft_version: metadata.minecraft_version.clone(),
        loader: metadata.loader.clone(),
        loader_version: metadata.loader_version.clone(),
    };

    if !source_exists {
        errors.push(format!("La carpeta original de la instancia ya no existe en: {}. Es posible que el launcher externo haya movido o eliminado la instancia.", source.display()));
    } else {
        let version_ids = build_version_id_candidates(&metadata.version_id, &hints);
        searched_paths = minecraft_jar_candidates(&source, &version_ids, &redirect.source_launcher)
            .into_iter()
            .map(|p| p.display().to_string())
            .collect();

        match resolve_redirect_launch_context(
            &source,
            &metadata.version_id,
            &redirect.source_launcher,
            &hints,
        ) {
            Ok(ctx) => {
                version_json_found = true;
                version_json_path = Some(ctx.version_json_path.display().to_string());
                minecraft_jar_found = true;
                minecraft_jar_path = Some(ctx.minecraft_jar.display().to_string());
                let runtime =
                    parse_java_runtime_for_redirect(&ctx.version_json, &ctx.resolved_version_id);
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
    let auth_session = refresh_microsoft_token_if_needed(auth_session)
        .await
        .map_err(|e| format!("No se pudo refrescar el token de autenticación: {e}"))?;
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

    let hints = RedirectVersionHints {
        minecraft_version: metadata.minecraft_version.clone(),
        loader: metadata.loader.clone(),
        loader_version: metadata.loader_version.clone(),
    };

    let loader_is_vanilla = matches!(
        metadata.loader.trim().to_ascii_lowercase().as_str(),
        "" | "-" | "vanilla" | "desconocido" | "unknown"
    );

    let ctx = if loader_is_vanilla {
        match resolve_redirect_launch_context(
            &source_path,
            &metadata.version_id,
            &redirect.source_launcher,
            &hints,
        ) {
            Ok(ctx) => ctx,
            Err(_) => {
                ensure_redirect_cache_context(
                    &app,
                    &source_path,
                    &redirect.source_launcher,
                    &metadata.internal_uuid,
                    &metadata.version_id,
                    &hints,
                )
                .await?
            }
        }
    } else {
        ensure_redirect_cache_context(
            &app,
            &source_path,
            &redirect.source_launcher,
            &metadata.internal_uuid,
            &metadata.version_id,
            &hints,
        )
        .await?
    };
    touch_cache_entry_last_used(&app, &metadata.internal_uuid);
    let runtime = parse_java_runtime_for_redirect(&ctx.version_json, &ctx.resolved_version_id);

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

    let mut libraries_dirs = vec![ctx.libraries_dir.clone()];
    if let Some(system_root) = system_minecraft_root() {
        libraries_dirs.push(system_root.join("libraries"));
    }
    for launcher_root in launcher_roots_for_source(&redirect.source_launcher) {
        libraries_dirs.push(launcher_root.join("libraries"));
    }
    let libraries_dirs: Vec<PathBuf> = libraries_dirs.into_iter().filter(|p| p.is_dir()).collect();

    let mut classpath = build_classpath_multi(
        &ctx.version_json,
        &libraries_dirs,
        &ctx.versions_dir,
        &ctx.resolved_version_id,
    )?;
    let classpath_separator = if cfg!(target_os = "windows") {
        ";"
    } else {
        ":"
    };
    let loader_lower = metadata.loader.to_ascii_lowercase();
    if loader_lower != "vanilla" && !loader_lower.is_empty() {
        let has_loader_jar = classpath.to_ascii_lowercase().contains(&loader_lower);
        if !has_loader_jar {
            log::warn!("[REDIRECT] Loader jar no encontrado en classpath, buscando manualmente...");
            if let Some(loader_jar) = find_loader_jar_in_dirs(&libraries_dirs, &loader_lower) {
                classpath.push_str(classpath_separator);
                classpath.push_str(&loader_jar.display().to_string());
            }
        }
    }
    let classpath_entry_count = classpath.split(classpath_separator).count();
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
        classpath_separator: classpath_separator.to_string(),
        library_directory: ctx.libraries_dir.display().to_string(),
        natives_dir: natives_dir.display().to_string(),
        launcher_name: "Interface-2".to_string(),
        launcher_version: env!("CARGO_PKG_VERSION").to_string(),
        auth_player_name: auth_session.profile_name.clone(),
        auth_uuid: auth_session.profile_id.clone(),
        auth_access_token: auth_session.minecraft_access_token.clone(),
        user_type: "msa".to_string(),
        user_properties: "{}".to_string(),
        version_name: ctx.resolved_version_id.clone(),
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

    log::info!("[REDIRECT] === DIAGNÓSTICO DE LOADER ===");
    log::info!("[REDIRECT] loader:          {}", metadata.loader);
    log::info!("[REDIRECT] loader_version:  {}", metadata.loader_version);
    log::info!("[REDIRECT] version_id:      {}", ctx.resolved_version_id);
    log::info!("[REDIRECT] mainClass:       {}", resolved.main_class);
    log::info!("[REDIRECT] game_dir:        {}", ctx.game_dir.display());
    log::info!(
        "[REDIRECT] mods/:           {}",
        ctx.game_dir.join("mods").exists()
    );
    log::info!(
        "[REDIRECT] config/:         {}",
        ctx.game_dir.join("config").exists()
    );
    log::info!(
        "[REDIRECT] shaderpacks/:    {}",
        ctx.game_dir.join("shaderpacks").exists()
    );
    log::info!("[REDIRECT] libs en cp:      {}", classpath_entry_count);

    if resolved.main_class == "net.minecraft.client.main.Main"
        && !matches!(
            metadata.loader.to_ascii_lowercase().as_str(),
            "vanilla" | "" | "-"
        )
    {
        log::error!(
            "[REDIRECT] ALERTA: mainClass es vanilla pero loader es {}. El loader no se activará. Verifica que el version.json del loader se resolvió correctamente.",
            metadata.loader
        );
    }
    log::info!("[REDIRECT] ==============================");

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

fn is_loader_vanilla(loader: &str) -> bool {
    matches!(
        loader.trim().to_ascii_lowercase().as_str(),
        "" | "-" | "vanilla" | "desconocido" | "unknown"
    )
}

fn repair_loader_version(metadata: &mut InstanceMetadata) -> Option<String> {
    if !metadata.loader_version.trim().is_empty() && metadata.loader_version != "-" {
        return None;
    }
    if let Some((_, detected_version)) = detect_loader_from_version_id(&metadata.version_id) {
        metadata.loader_version = detected_version.clone();
        return Some(format!(
            "loader_version detectado desde version_id: {}",
            detected_version
        ));
    }
    None
}

#[tauri::command]
pub async fn repair_instance(
    app: AppHandle,
    instance_root: String,
) -> Result<RepairInstanceResult, String> {
    let instance_path = PathBuf::from(&instance_root);
    let mut metadata = get_instance_metadata(instance_root.clone())?;
    let mut changes_made = Vec::new();
    let mut errors = Vec::new();

    let _ = app.emit(
        "repair_instance_progress",
        json!({
            "instanceRoot": instance_root,
            "stage": "analyzing",
            "message": "Analizando metadata de instancia..."
        }),
    );

    if let Some(change) = repair_loader_version(&mut metadata) {
        changes_made.push(change);
    }

    if metadata.state.eq_ignore_ascii_case("REDIRECT") {
        match read_redirect_file(&instance_path) {
            Ok(redirect) => {
                let source_path = PathBuf::from(&redirect.source_path);
                if !source_path.exists() {
                    errors.push(format!(
                        "source_path no existe para REDIRECT: {}",
                        source_path.display()
                    ));
                } else {
                    if !is_loader_vanilla(&metadata.loader)
                        && !metadata
                            .version_id
                            .to_ascii_lowercase()
                            .contains(&metadata.loader.to_ascii_lowercase())
                    {
                        let repaired_version = resolve_effective_version_id(
                            &source_path,
                            &metadata.minecraft_version,
                            &metadata.loader,
                            &metadata.loader_version,
                            &redirect.source_launcher,
                        );
                        if repaired_version != metadata.version_id {
                            changes_made.push(format!(
                                "version_id corregido: {} -> {}",
                                metadata.version_id, repaired_version
                            ));
                            metadata.version_id = repaired_version;
                        }
                    }

                    let hints = RedirectVersionHints {
                        minecraft_version: metadata.minecraft_version.clone(),
                        loader: metadata.loader.clone(),
                        loader_version: metadata.loader_version.clone(),
                    };
                    let _ = app.emit(
                        "repair_instance_progress",
                        json!({
                            "instanceRoot": instance_root,
                            "stage": "prewarm_redirect",
                            "message": "Reconstruyendo caché REDIRECT..."
                        }),
                    );
                    if let Err(err) = prewarm_redirect_runtime(
                        &app,
                        &source_path,
                        &redirect.source_launcher,
                        &metadata.internal_uuid,
                        &metadata.version_id,
                        &hints,
                    )
                    .await
                    {
                        errors.push(format!("No se pudo prewarm REDIRECT: {err}"));
                    } else {
                        changes_made.push("Runtime REDIRECT prewarm completado".to_string());
                    }
                }
            }
            Err(err) => errors.push(format!(".redirect.json inválido o ausente: {err}")),
        }
    } else if metadata.state.eq_ignore_ascii_case("READY")
        || metadata.state.eq_ignore_ascii_case("IMPORTED")
    {
        let launcher_root = instance_path
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| "No se pudo resolver launcher_root para repair_instance".to_string())?;
        let minecraft_root = if instance_path.join("minecraft").is_dir() {
            instance_path.join("minecraft")
        } else {
            instance_path.clone()
        };

        let required_java = match metadata.required_java_major {
            21 => JavaRuntime::Java21,
            17 => JavaRuntime::Java17,
            _ => JavaRuntime::Java8,
        };

        let mut logs = Vec::new();
        let _ = app.emit(
            "repair_instance_progress",
            json!({
                "instanceRoot": instance_root,
                "stage": "rebuild_runtime",
                "message": "Reinstalando runtime/loader de la instancia..."
            }),
        );
        match ensure_embedded_java(launcher_root, required_java, &mut logs).and_then(|java_exec| {
            build_instance_structure(
                &instance_path,
                &minecraft_root,
                &metadata.minecraft_version,
                &metadata.loader,
                &metadata.loader_version,
                &java_exec,
                &mut logs,
                &mut |_progress| {},
            )
            .map(|version_id| (java_exec, version_id))
        }) {
            Ok((java_exec, version_id)) => {
                if metadata.version_id != version_id {
                    changes_made.push(format!(
                        "version_id reconstruido: {} -> {}",
                        metadata.version_id, version_id
                    ));
                    metadata.version_id = version_id;
                }
                metadata.java_path = java_exec.display().to_string();
                changes_made.push("Runtime/loader reinstalado correctamente".to_string());
            }
            Err(err) => errors.push(format!("No se pudo reconstruir runtime: {err}")),
        }
    }

    if errors.is_empty() || !changes_made.is_empty() {
        write_instance_metadata(&instance_path, &metadata)?;
    }

    Ok(RepairInstanceResult {
        repaired: errors.is_empty() && !changes_made.is_empty(),
        changes_made,
        errors,
        final_state: metadata.state,
    })
}

#[tauri::command]
pub async fn repair_all_instances(app: AppHandle) -> Result<Vec<RepairInstanceResult>, String> {
    let instances_root = crate::app::settings_service::resolve_instances_root(&app)?;
    let mut results = Vec::new();

    let entries = fs::read_dir(&instances_root).map_err(|err| {
        format!(
            "No se pudo leer instances_root {}: {err}",
            instances_root.display()
        )
    })?;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let instance_root = path.display().to_string();
        let metadata = match get_instance_metadata(instance_root.clone()) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };

        let should_repair = !metadata.state.eq_ignore_ascii_case("READY")
            || (!is_loader_vanilla(&metadata.loader)
                && !metadata
                    .version_id
                    .to_ascii_lowercase()
                    .contains(&metadata.loader.to_ascii_lowercase()))
            || metadata.loader_version.trim().is_empty()
            || metadata.loader_version == "-";

        if !should_repair {
            continue;
        }

        match repair_instance(app.clone(), instance_root).await {
            Ok(result) => results.push(result),
            Err(err) => results.push(RepairInstanceResult {
                repaired: false,
                changes_made: Vec::new(),
                errors: vec![err],
                final_state: metadata.state,
            }),
        }
    }

    Ok(results)
}
