use std::{
    collections::{HashSet, VecDeque},
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, OnceLock,
    },
    time::SystemTime,
};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use crate::{
    domain::java::java_requirement::determine_required_java,
    domain::models::instance::InstanceMetadata,
    domain::models::java::JavaRuntime,
    infrastructure::filesystem::paths::sanitize_path_segment,
    services::{instance_builder::build_instance_structure, java_installer::ensure_embedded_java},
};

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedInstance {
    id: String,
    name: String,
    source_launcher: String,
    source_path: String,
    minecraft_version: String,
    loader: String,
    loader_version: String,
    format: String,
    icon_path: Option<String>,
    mods_count: Option<u32>,
    size_mb: Option<u64>,
    last_played: Option<String>,
    importable: bool,
    import_warnings: Vec<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRequest {
    detected_instance_id: String,
    source_path: String,
    target_name: String,
    target_group: String,
    minecraft_version: String,
    loader: String,
    loader_version: String,
    ram_mb: u32,
    copy_mods: bool,
    copy_worlds: bool,
    copy_resourcepacks: bool,
    copy_screenshots: bool,
    copy_logs: bool,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportActionRequest {
    detected_instance_id: String,
    source_path: String,
    target_name: String,
    target_group: String,
    minecraft_version: String,
    loader: String,
    loader_version: String,
    source_launcher: String,
    action: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportActionResult {
    success: bool,
    target_name: String,
    target_path: Option<String>,
    error: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportActionBatchFailure {
    instance_id: String,
    target_name: String,
    error: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportActionBatchResult {
    success: bool,
    action: String,
    total: usize,
    success_count: usize,
    failure_count: usize,
    failures: Vec<ImportActionBatchFailure>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportFocusStatus {
    key: String,
    label: String,
    status: String,
}

fn emit_action_progress(
    app: &AppHandle,
    request: &ImportActionRequest,
    action: &str,
    completed: usize,
    total: usize,
    step_index: usize,
    total_steps: usize,
    step: &str,
    message: String,
    checkpoints: Option<Vec<ImportFocusStatus>>,
) {
    let _ = app.emit(
        "import_execution_progress",
        serde_json::json!({
            "instanceId": request.detected_instance_id,
            "instanceName": request.target_name,
            "action": action,
            "step": step,
            "stepIndex": step_index,
            "totalSteps": total_steps,
            "completed": completed,
            "total": total,
            "message": message,
            "checkpoints": checkpoints
        }),
    );
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanProgressEvent {
    stage: String,
    message: String,
    found_so_far: usize,
    current_path: String,
    progress_percent: u8,
    total_targets: usize,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShortcutRedirect {
    source_path: String,
    source_launcher: String,
}

#[derive(Default)]
struct DetectionMeta {
    minecraft_version: Option<String>,
    loader: Option<String>,
    loader_version: Option<String>,
    format: Option<String>,
    importable: bool,
}

fn runtime_name(runtime: JavaRuntime) -> &'static str {
    match runtime {
        JavaRuntime::Java8 => "java8",
        JavaRuntime::Java17 => "java17",
        JavaRuntime::Java21 => "java21",
    }
}

fn detect_source_minecraft_dir(source_root: &Path) -> Option<PathBuf> {
    let preferred = [
        source_root.join("minecraft"),
        source_root.join(".minecraft"),
    ];
    if let Some(path) = preferred.into_iter().find(|candidate| candidate.is_dir()) {
        return Some(path);
    }

    if source_root.join("versions").is_dir() {
        return Some(source_root.to_path_buf());
    }

    let mut fallback: Option<(u8, PathBuf)> = None;
    let Ok(entries) = fs::read_dir(source_root) else {
        return None;
    };

    for entry in entries.flatten() {
        let candidate = entry.path();
        if !candidate.is_dir() {
            continue;
        }

        let mut score = 0u8;
        if candidate.join("versions").is_dir() {
            score = score.saturating_add(5);
        }
        if candidate.join("mods").is_dir() {
            score = score.saturating_add(2);
        }
        if candidate.join("assets").is_dir() {
            score = score.saturating_add(2);
        }
        if candidate.join("options.txt").is_file() {
            score = score.saturating_add(1);
        }
        if score == 0 {
            continue;
        }

        if fallback
            .as_ref()
            .map(|(best, _)| score > *best)
            .unwrap_or(true)
        {
            fallback = Some((score, candidate));
        }
    }

    fallback.map(|(_, path)| path)
}

fn normalize_import_layout(instance_root: &Path, source_root: &Path) -> Result<PathBuf, String> {
    let minecraft_root = instance_root.join("minecraft");
    if minecraft_root.is_dir() {
        return Ok(minecraft_root);
    }

    let dot_minecraft_root = instance_root.join(".minecraft");
    if dot_minecraft_root.is_dir() {
        fs::rename(&dot_minecraft_root, &minecraft_root).map_err(|err| {
            format!(
                "No se pudo normalizar .minecraft -> minecraft en {}: {err}",
                instance_root.display()
            )
        })?;
        return Ok(minecraft_root);
    }

    fs::create_dir_all(&minecraft_root).map_err(|err| {
        format!(
            "No se pudo crear carpeta minecraft en {}: {err}",
            instance_root.display()
        )
    })?;

    if let Some(source_mc) = detect_source_minecraft_dir(source_root) {
        let mut copied = 0usize;
        copy_dir_recursive_limited(&source_mc, &minecraft_root, &mut copied, None)?;
    }

    Ok(minecraft_root)
}

fn copy_dir_recursive_limited(
    src: &Path,
    dst: &Path,
    copied: &mut usize,
    max_files: Option<usize>,
) -> Result<(), String> {
    if max_files.is_some_and(|max| *copied >= max) || !src.exists() {
        return Ok(());
    }

    fs::create_dir_all(dst).map_err(|err| {
        format!(
            "No se pudo crear carpeta de destino {}: {err}",
            dst.display()
        )
    })?;

    let entries =
        fs::read_dir(src).map_err(|err| format!("No se pudo leer {}: {err}", src.display()))?;

    for entry in entries.flatten() {
        if max_files.is_some_and(|max| *copied >= max) {
            break;
        }

        let path = entry.path();
        let target = dst.join(entry.file_name());

        if path.is_dir() {
            let dir_name = path
                .file_name()
                .and_then(|value| value.to_str())
                .map(|value| value.to_ascii_lowercase())
                .unwrap_or_default();
            if ["cache", "temp", "tmp", "natives"].contains(&dir_name.as_str()) {
                continue;
            }
            copy_dir_recursive_limited(&path, &target, copied, max_files)?;
            continue;
        }

        fs::copy(&path, &target).map_err(|err| {
            format!(
                "No se pudo copiar {} -> {}: {err}",
                path.display(),
                target.display()
            )
        })?;
        *copied += 1;
    }

    Ok(())
}

fn finalize_import_runtime(
    app: &AppHandle,
    instance_root: &Path,
    source_root: &Path,
    metadata: &mut InstanceMetadata,
) -> Result<(), String> {
    let launcher_root = instance_root
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| {
            format!(
                "No se pudo resolver launcher root desde {}",
                instance_root.display()
            )
        })?;
    let minecraft_root = normalize_import_layout(instance_root, source_root)?;

    let required_java = determine_required_java(&metadata.minecraft_version, &metadata.loader)?;
    let mut logs = vec![format!(
        "[IMPORT] Preparando runtime oficial para {} ({})",
        metadata.name, metadata.version_id
    )];
    let java_exec = ensure_embedded_java(launcher_root, required_java, &mut logs)?;
    let effective_version_id = build_instance_structure(
        instance_root,
        &minecraft_root,
        &metadata.minecraft_version,
        &metadata.loader,
        &metadata.loader_version,
        &java_exec,
        &mut logs,
        &mut |_progress| {},
    )?;

    metadata.version_id = effective_version_id;
    metadata.java_path = java_exec.display().to_string();
    metadata.java_runtime = runtime_name(required_java).to_string();
    metadata.java_version = format!("{}.0.x", required_java.major());
    metadata.required_java_major = u32::from(required_java.major());
    metadata.state = "READY".to_string();

    let _ = app.emit(
        "import_execution_progress",
        serde_json::json!({
            "instanceName": metadata.name,
            "step": "runtime_ready",
            "message": "Runtime oficial, assets y loader verificados para importación."
        }),
    );

    Ok(())
}

static CANCEL_IMPORT: OnceLock<Arc<AtomicBool>> = OnceLock::new();

const INSTANCE_IDENTIFIER_FILES: &[&str] = &[
    "minecraftinstance.json",
    "mmc-pack.json",
    "manifest.json",
    "profile.json",
    "instance.cfg",
    ".curseclient",
];

const INSTANCE_MINECRAFT_DIRS: &[&str] = &[".minecraft", "minecraft"];
const INSTANCE_HINT_KEYWORDS: &[&str] = &[
    "instancias",
    "instancia",
    "instances",
    "instance",
    "launcher",
    "minecraft",
    ".minecraft",
    "modpacks",
    "prism",
    "multimc",
    "curseforge",
    "curse",
    "modrinth",
    "forge",
    "neoforge",
    "fabric",
    "quilt",
    "gdlauncher",
    "atlauncher",
    "polymc",
    "mmc",
];

const SCAN_SKIP_DIR_NAMES: &[&str] = &[
    "node_modules",
    "target",
    ".git",
    ".cache",
    ".cargo",
    ".rustup",
    "Program Files",
    "Program Files (x86)",
    "Windows",
    "System Volume Information",
];

const MAX_DISCOVERY_VISITED_DIRS: usize = 2_000;
const MAX_ROOT_CHILDREN_TO_SCAN: usize = 180;
const SCAN_PROGRESS_EMIT_INTERVAL: usize = 25;
const DISCOVERY_SCAN_DEPTH: usize = 3;
const DISCOVERY_MAX_CANDIDATES_PER_ROOT: usize = 64;

fn known_paths() -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();

    #[cfg(target_os = "windows")]
    {
        if let Ok(user_profile) = std::env::var("USERPROFILE") {
            out.push((
                "CurseForge".to_string(),
                PathBuf::from(&user_profile).join("curseforge/minecraft/instances"),
            ));
            out.push((
                "CurseForge".to_string(),
                PathBuf::from(&user_profile).join("curseforge/minecraft/Install"),
            ));
        }
        if let Ok(appdata) = std::env::var("APPDATA") {
            out.push((
                "CurseForge".to_string(),
                PathBuf::from(&appdata).join("CurseForge/Minecraft/Instances"),
            ));
            out.push((
                "CurseForge".to_string(),
                PathBuf::from(&appdata).join("CurseForge/Minecraft/Install"),
            ));
            out.push((
                "Modrinth".to_string(),
                PathBuf::from(&appdata).join("com.modrinth.theseus/profiles"),
            ));
            out.push((
                "Prism".to_string(),
                PathBuf::from(&appdata).join("PrismLauncher/instances"),
            ));
            out.push((
                "MultiMC".to_string(),
                PathBuf::from(&appdata).join("MultiMC/instances"),
            ));
            out.push((
                "Mojang Official".to_string(),
                PathBuf::from(&appdata).join(".minecraft"),
            ));
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            out.push((
                "Prism".to_string(),
                PathBuf::from(&home).join("Library/Application Support/PrismLauncher/instances"),
            ));
            out.push((
                "Modrinth".to_string(),
                PathBuf::from(&home)
                    .join("Library/Application Support/com.modrinth.theseus/profiles"),
            ));
            out.push((
                "Mojang Official".to_string(),
                PathBuf::from(&home).join("Library/Application Support/minecraft"),
            ));
            out.push((
                "CurseForge".to_string(),
                PathBuf::from(&home)
                    .join("Library/Application Support/curseforge/minecraft/instances"),
            ));
            out.push((
                "CurseForge".to_string(),
                PathBuf::from(&home)
                    .join("Library/Application Support/curseforge/minecraft/Install"),
            ));
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Ok(home) = std::env::var("HOME") {
            out.push((
                "Prism".to_string(),
                PathBuf::from(&home).join(".local/share/PrismLauncher/instances"),
            ));
            out.push((
                "Modrinth".to_string(),
                PathBuf::from(&home).join(".config/com.modrinth.theseus/profiles"),
            ));
            out.push((
                "MultiMC".to_string(),
                PathBuf::from(&home).join(".local/share/MultiMC/instances"),
            ));
            out.push((
                "Mojang Official".to_string(),
                PathBuf::from(&home).join(".minecraft"),
            ));
            out.push((
                "CurseForge".to_string(),
                PathBuf::from(&home).join(".local/share/curseforge/minecraft/instances"),
            ));
            out.push((
                "CurseForge".to_string(),
                PathBuf::from(&home).join(".local/share/curseforge/minecraft/install"),
            ));
            out.push((
                "CurseForge".to_string(),
                PathBuf::from(&home).join(".local/share/curseforge/minecraft/Install"),
            ));
        }
    }

    out
}

fn has_instance_markers(path: &Path) -> bool {
    INSTANCE_IDENTIFIER_FILES
        .iter()
        .any(|file| path.join(file).is_file())
}

fn has_valid_versions_layout(path: &Path) -> bool {
    let versions_dir = path.join("versions");
    if !versions_dir.is_dir() {
        return false;
    }

    fs::read_dir(&versions_dir)
        .ok()
        .map(|entries| {
            entries.flatten().any(|entry| {
                let version_id = entry.file_name().to_string_lossy().to_string();
                if version_id.is_empty() {
                    return false;
                }
                let version_root = entry.path();
                version_root.is_dir()
                    && version_root.join(format!("{version_id}.json")).is_file()
                    && version_root.join(format!("{version_id}.jar")).is_file()
            })
        })
        .unwrap_or(false)
}

fn has_required_instance_layout(path: &Path) -> bool {
    if has_instance_markers(path) {
        return true;
    }

    if is_likely_instance_container(path) {
        return false;
    }

    let game_dir = detect_source_minecraft_dir(path).unwrap_or_else(|| path.to_path_buf());
    let has_modded_state = game_dir.join("mods").is_dir()
        || game_dir.join("saves").is_dir()
        || game_dir.join("config").is_dir();
    let has_playable_state = game_dir.join("options.txt").is_file() || has_modded_state;
    let has_runtime_layout = has_valid_versions_layout(&game_dir)
        && (game_dir.join("assets").is_dir() || game_dir.join("libraries").is_dir());

    if has_modded_state && has_runtime_layout {
        return true;
    }

    let is_minecraft_root_dir = path
        .file_name()
        .and_then(|value| value.to_str())
        .map(|name| {
            name.eq_ignore_ascii_case(".minecraft") || name.eq_ignore_ascii_case("minecraft")
        })
        .unwrap_or(false);

    if is_minecraft_root_dir {
        return false;
    }

    has_playable_state && has_runtime_layout
}

fn is_likely_instance_container(path: &Path) -> bool {
    let dir_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();
    let known_container_names = ["instances", "profiles", "modpacks", "install", "roaming"];

    if known_container_names
        .iter()
        .any(|name| dir_name == *name || dir_name.contains(name))
    {
        return true;
    }

    let Ok(entries) = fs::read_dir(path) else {
        return false;
    };

    let mut child_instances = 0usize;
    for entry in entries.flatten() {
        let child = entry.path();
        if !child.is_dir() {
            continue;
        }

        let child_has_markers = has_instance_markers(&child)
            || child.join("minecraft").is_dir()
            || child.join(".minecraft").is_dir();
        if child_has_markers {
            child_instances += 1;
            if child_instances >= 2 {
                return true;
            }
        }
    }

    false
}

fn directory_name_looks_like_container(path: &Path) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(|value| {
            let lower = value.to_ascii_lowercase();
            INSTANCE_HINT_KEYWORDS
                .iter()
                .any(|keyword| lower.contains(keyword))
        })
        .unwrap_or(false)
}

fn external_search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    #[cfg(target_os = "windows")]
    {
        for env_name in ["APPDATA", "LOCALAPPDATA", "USERPROFILE", "PUBLIC"] {
            if let Ok(value) = std::env::var(env_name) {
                roots.push(PathBuf::from(value));
            }
        }

        for drive in b'A'..=b'Z' {
            let root = PathBuf::from(format!("{}:\\", drive as char));
            if root.exists() {
                roots.push(root);
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        for candidate in ["/media", "/mnt", "/run/media", "/Volumes", "/opt", "/srv"] {
            let path = PathBuf::from(candidate);
            if path.exists() {
                roots.push(path);
            }
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        let home = PathBuf::from(home);
        roots.push(home.clone());
        roots.push(home.join("Desktop"));
        roots.push(home.join("Documents"));
        roots.push(home.join("Downloads"));
        roots.push(home.join("Games"));
        roots.push(home.join(".minecraft"));
        roots.push(home.join(".local/share"));
        roots.push(home.join(".config"));
        roots.push(home.join("AppData/Roaming"));
        roots.push(home.join("AppData/Local"));
    }

    roots
}

fn discover_keyword_scan_paths(
    base: &Path,
    max_depth: usize,
    max_candidates: usize,
) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back((base.to_path_buf(), 0usize));

    while let Some((current, depth)) = queue.pop_front() {
        if visited.len() >= MAX_DISCOVERY_VISITED_DIRS {
            break;
        }
        if found.len() >= max_candidates {
            break;
        }

        let canonical = fs::canonicalize(&current).unwrap_or(current.clone());
        if !visited.insert(canonical) {
            continue;
        }

        if has_required_instance_layout(&current) {
            found.push(current.clone());
        }

        if depth >= max_depth {
            continue;
        }

        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && !should_skip_scan_dir(&path) {
                queue.push_back((path, depth + 1));
            }
        }
    }

    found
}

fn should_skip_scan_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(|value| {
            if value.starts_with('.') {
                return true;
            }
            SCAN_SKIP_DIR_NAMES
                .iter()
                .any(|skip| value.eq_ignore_ascii_case(skip))
        })
        .unwrap_or(false)
}

fn known_and_discovered_paths() -> Vec<(String, PathBuf)> {
    let mut out = known_paths();
    let mut seen = HashSet::new();

    for (_, path) in &out {
        seen.insert(fs::canonicalize(path).unwrap_or(path.clone()));
    }

    for base in external_search_roots() {
        if !base.exists() || !base.is_dir() {
            continue;
        }

        for discovered in discover_keyword_scan_paths(
            &base,
            DISCOVERY_SCAN_DEPTH,
            DISCOVERY_MAX_CANDIDATES_PER_ROOT,
        ) {
            let canonical = fs::canonicalize(&discovered).unwrap_or(discovered.clone());
            if seen.insert(canonical) {
                out.push((detect_launcher_from_path(&discovered), discovered));
            }
        }
    }

    out
}

fn detect_launcher_from_path(path: &Path) -> String {
    let lower = path.to_string_lossy().to_ascii_lowercase();
    if lower.contains("modrinth") || lower.contains("theseus") {
        return "Modrinth".to_string();
    }
    if lower.contains("prism") {
        return "Prism".to_string();
    }
    if lower.contains("multimc") || lower.contains("mmc") {
        return "MultiMC".to_string();
    }
    if lower.contains("curseforge") || lower.contains("curse") {
        return "CurseForge".to_string();
    }
    if lower.contains("tlauncher") {
        return "TLauncher".to_string();
    }
    if lower.contains("\\.minecraft") || lower.ends_with("/minecraft") {
        return "Mojang Official".to_string();
    }
    "Descubierto".to_string()
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn normalize_loader(loader: &str) -> String {
    let normalized = loader.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "quilit" => "quilt".to_string(),
        _ => normalized,
    }
}

fn detect_loader_from_version_id(version_id: &str) -> Option<(String, String)> {
    let normalized = version_id.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }

    let patterns: [(&str, &str); 4] = [
        ("fabric-loader-", "fabric"),
        ("quilt-loader-", "quilt"),
        ("neoforge-", "neoforge"),
        ("forge-", "forge"),
    ];

    for (token, loader_name) in patterns {
        if let Some(pos) = normalized.find(token) {
            let raw = &normalized[(pos + token.len())..];
            let version = raw.split(['+', '-', '_']).next().unwrap_or("").trim();
            return Some((
                loader_name.to_string(),
                if version.is_empty() {
                    "-".to_string()
                } else {
                    version.to_string()
                },
            ));
        }
    }

    None
}

fn detect_loader_from_versions_dir(path: &Path) -> Option<(String, String)> {
    let versions_candidates = [
        path.join("versions"),
        path.join(".minecraft/versions"),
        path.join("minecraft/versions"),
    ];
    for versions_dir in versions_candidates {
        if !versions_dir.is_dir() {
            continue;
        }
        let mut best: Option<(String, String)> = None;
        if let Ok(entries) = fs::read_dir(&versions_dir) {
            for entry in entries.flatten() {
                let version_id = entry.file_name().to_string_lossy().to_string();
                if let Some(loader) = detect_loader_from_version_id(&version_id) {
                    best = Some(loader);
                    break;
                }
            }
        }
        if best.is_some() {
            return best;
        }
    }
    None
}

fn resolve_shortcut_version_id(
    minecraft_version: &str,
    loader: &str,
    loader_version: &str,
) -> String {
    let mc = minecraft_version.trim();
    let loader = normalize_loader(loader);
    let loader_version = loader_version.trim();

    if mc.is_empty() || mc.eq_ignore_ascii_case("desconocida") {
        return minecraft_version.to_string();
    }

    match loader.as_str() {
        "fabric" if !loader_version.is_empty() && loader_version != "-" => {
            format!("fabric-loader-{loader_version}-{mc}")
        }
        "quilt" if !loader_version.is_empty() && loader_version != "-" => {
            format!("quilt-loader-{loader_version}-{mc}")
        }
        "forge" if !loader_version.is_empty() && loader_version != "-" => {
            format!("{mc}-forge-{loader_version}")
        }
        "neoforge" if !loader_version.is_empty() && loader_version != "-" => {
            format!("{mc}-neoforge-{loader_version}")
        }
        _ => mc.to_string(),
    }
}

fn system_minecraft_root() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return Some(PathBuf::from(appdata).join(".minecraft"));
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            return Some(PathBuf::from(home).join("Library/Application Support/minecraft"));
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Ok(home) = std::env::var("HOME") {
            return Some(PathBuf::from(home).join(".minecraft"));
        }
    }

    None
}

fn launcher_roots_for_source(source_launcher: &str) -> Vec<PathBuf> {
    let lower = source_launcher.trim().to_ascii_lowercase();
    let matches_all = lower.is_empty() || lower == "auto detectado";

    known_paths()
        .into_iter()
        .filter(|(name, path)| {
            if matches_all {
                return true;
            }

            let name_lower = name.to_ascii_lowercase();
            let path_lower = path.to_string_lossy().to_ascii_lowercase();
            if name_lower.contains(&lower) || path_lower.contains(&lower) {
                return true;
            }

            (lower.contains("curseforge") || lower.contains("curse"))
                && path_lower.contains("curseforge")
        })
        .map(|(_, path)| path)
        .collect()
}

fn find_loader_version_id_from_external_paths(
    source_path: &Path,
    source_launcher: &str,
    minecraft_version: &str,
    loader: &str,
) -> Option<String> {
    let loader_lower = loader.to_ascii_lowercase();
    let mc_lower = minecraft_version.to_ascii_lowercase();
    let source_game_dir = detect_source_minecraft_dir(source_path);
    let mut versions_roots = vec![
        source_path.join("versions"),
        source_path.join(".minecraft/versions"),
    ];
    if let Some(game_dir) = source_game_dir {
        versions_roots.push(game_dir.join("versions"));
    }

    let roots = versions_roots
        .into_iter()
        .chain(
            launcher_roots_for_source(source_launcher)
                .into_iter()
                .map(|root| root.join("versions")),
        )
        .chain(
            system_minecraft_root()
                .into_iter()
                .map(|root| root.join("versions")),
        );

    for versions_dir in roots {
        if !versions_dir.is_dir() {
            continue;
        }
        let Ok(entries) = fs::read_dir(&versions_dir) else {
            continue;
        };

        for entry in entries.flatten() {
            let version_id = entry.file_name().to_string_lossy().to_string();
            let id_lower = version_id.to_ascii_lowercase();
            if !id_lower.contains(&loader_lower) || !id_lower.contains(&mc_lower) {
                continue;
            }
            let json_path = versions_dir
                .join(&version_id)
                .join(format!("{version_id}.json"));
            if json_path.is_file() {
                return Some(version_id);
            }
        }
    }

    None
}

pub(crate) fn resolve_effective_version_id(
    source_root: &Path,
    minecraft_version: &str,
    loader: &str,
    loader_version: &str,
    source_launcher: &str,
) -> String {
    let expected = resolve_shortcut_version_id(minecraft_version, loader, loader_version);
    let expected_lower = expected.to_ascii_lowercase();
    let mc_lower = minecraft_version.trim().to_ascii_lowercase();
    let loader_lower = normalize_loader(loader);

    let mut version_roots = vec![
        source_root.join("versions"),
        source_root.join(".minecraft/versions"),
    ];
    if let Some(game_dir) = detect_source_minecraft_dir(source_root) {
        version_roots.push(game_dir.join("versions"));
    }
    for root in launcher_roots_for_source(source_launcher) {
        version_roots.push(root.join("versions"));
    }
    if let Some(system_root) = system_minecraft_root() {
        version_roots.push(system_root.join("versions"));
    }

    let mut fallback_mc_match: Option<String> = None;

    for versions_dir in version_roots {
        if !versions_dir.is_dir() {
            continue;
        }
        let Ok(entries) = fs::read_dir(&versions_dir) else {
            continue;
        };

        for entry in entries.flatten() {
            let version_id = entry.file_name().to_string_lossy().to_string();
            let version_lower = version_id.to_ascii_lowercase();
            if version_lower == expected_lower {
                return version_id;
            }

            if !mc_lower.is_empty() && !version_lower.contains(&mc_lower) {
                continue;
            }

            if loader_lower == "vanilla" || loader_lower == "desconocido" || loader_lower.is_empty()
            {
                if !version_lower.contains("forge")
                    && !version_lower.contains("fabric")
                    && !version_lower.contains("quilt")
                    && !version_lower.contains("neoforge")
                    && versions_dir
                        .join(&version_id)
                        .join(format!("{version_id}.json"))
                        .is_file()
                {
                    return version_id;
                }
                continue;
            }

            if version_lower.contains(&loader_lower)
                && versions_dir
                    .join(&version_id)
                    .join(format!("{version_id}.json"))
                    .is_file()
            {
                return version_id;
            }

            if fallback_mc_match.is_none() {
                fallback_mc_match = Some(version_id);
            }
        }
    }

    fallback_mc_match.unwrap_or(expected)
}

fn version_id_contains_loader(version_id: &str, loader: &str) -> bool {
    let loader_lower = normalize_loader(loader);
    if loader_lower.is_empty() || loader_lower == "vanilla" || loader_lower == "desconocido" {
        return true;
    }
    version_id.to_ascii_lowercase().contains(&loader_lower)
}

fn is_unknown_loader(loader: &str) -> bool {
    let normalized = normalize_loader(loader);
    matches!(normalized.as_str(), "" | "-" | "desconocido" | "unknown")
}

fn detect_from_manifest(path: &Path) -> DetectionMeta {
    let mut meta = DetectionMeta::default();

    let prism_manifest = path.join("minecraftinstance.json");
    if let Some(json) = read_json(&prism_manifest) {
        meta.importable = true;
        meta.format = Some("prism".to_string());
        meta.minecraft_version =
            json.get("components")
                .and_then(|c| c.as_array())
                .and_then(|components| {
                    components.iter().find_map(|component| {
                        let uid = component.get("uid")?.as_str()?;
                        if uid == "net.minecraft" {
                            component
                                .get("version")
                                .and_then(|v| v.as_str())
                                .map(ToOwned::to_owned)
                        } else {
                            None
                        }
                    })
                });

        if let Some((loader, version)) =
            json.get("components")
                .and_then(|c| c.as_array())
                .and_then(|components| {
                    components.iter().find_map(|component| {
                        let uid = component.get("uid")?.as_str()?.to_lowercase();
                        let version = component
                            .get("version")
                            .and_then(|v| v.as_str())
                            .unwrap_or("-")
                            .to_string();
                        if uid.contains("fabric") {
                            Some(("fabric".to_string(), version))
                        } else if uid.contains("forge") && !uid.contains("neoforge") {
                            Some(("forge".to_string(), version))
                        } else if uid.contains("neoforge") {
                            Some(("neoforge".to_string(), version))
                        } else if uid.contains("quilt") {
                            Some(("quilt".to_string(), version))
                        } else {
                            None
                        }
                    })
                })
        {
            meta.loader = Some(loader);
            meta.loader_version = Some(version);
        }

        return meta;
    }

    let multimc_manifest = path.join("mmc-pack.json");
    if let Some(json) = read_json(&multimc_manifest) {
        meta.importable = true;
        meta.format = Some("multimc".to_string());
        meta.minecraft_version =
            json.get("components")
                .and_then(|c| c.as_array())
                .and_then(|components| {
                    components.iter().find_map(|component| {
                        let uid = component.get("uid")?.as_str()?;
                        if uid == "net.minecraft" {
                            component
                                .get("version")
                                .and_then(|v| v.as_str())
                                .map(ToOwned::to_owned)
                        } else {
                            None
                        }
                    })
                });
        if let Some((loader, version)) =
            json.get("components")
                .and_then(|c| c.as_array())
                .and_then(|components| {
                    components.iter().find_map(|component| {
                        let uid = component.get("uid")?.as_str()?.to_lowercase();
                        let version = component
                            .get("version")
                            .and_then(|v| v.as_str())
                            .unwrap_or("-")
                            .to_string();
                        if uid.contains("fabric") {
                            Some(("fabric".to_string(), version))
                        } else if uid.contains("forge") && !uid.contains("neoforge") {
                            Some(("forge".to_string(), version))
                        } else if uid.contains("neoforge") {
                            Some(("neoforge".to_string(), version))
                        } else if uid.contains("quilt") {
                            Some(("quilt".to_string(), version))
                        } else {
                            None
                        }
                    })
                })
        {
            meta.loader = Some(loader);
            meta.loader_version = Some(version);
        }
        return meta;
    }

    let modrinth_manifest = path.join("profile.json");
    if let Some(json) = read_json(&modrinth_manifest) {
        meta.importable = true;
        meta.format = Some("modrinth".to_string());
        meta.minecraft_version = json
            .get("game_version")
            .and_then(|v| v.as_str())
            .map(ToOwned::to_owned);
        meta.loader = json
            .get("loader")
            .and_then(|v| v.as_str())
            .map(ToOwned::to_owned);
        meta.loader_version = json
            .get("loader_version")
            .and_then(|v| v.as_str())
            .map(ToOwned::to_owned);
        return meta;
    }

    let curse_manifest = path.join("manifest.json");
    if let Some(json) = read_json(&curse_manifest) {
        meta.importable = true;
        meta.format = Some("curseforge".to_string());
        meta.minecraft_version = json
            .get("minecraft")
            .and_then(|minecraft| minecraft.get("version"))
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned);

        if let Some((loader, version)) = json
            .get("minecraft")
            .and_then(|minecraft| minecraft.get("modLoaders"))
            .and_then(|value| value.as_array())
            .and_then(|loaders| {
                loaders.iter().find_map(|entry| {
                    let id = entry.get("id")?.as_str()?.to_ascii_lowercase();
                    if id.starts_with("forge-") {
                        return Some((
                            "forge".to_string(),
                            id.trim_start_matches("forge-").to_string(),
                        ));
                    }
                    if id.starts_with("neoforge-") {
                        return Some((
                            "neoforge".to_string(),
                            id.trim_start_matches("neoforge-").to_string(),
                        ));
                    }
                    if id.starts_with("fabric-") {
                        return Some((
                            "fabric".to_string(),
                            id.trim_start_matches("fabric-").to_string(),
                        ));
                    }
                    if id.starts_with("quilt-") {
                        return Some((
                            "quilt".to_string(),
                            id.trim_start_matches("quilt-").to_string(),
                        ));
                    }
                    None
                })
            })
        {
            meta.loader = Some(loader);
            meta.loader_version = Some(version);
        }

        return meta;
    }

    if path.join(".curseclient").exists() {
        meta.importable = true;
        meta.format = Some("curseforge".to_string());
        return meta;
    }

    if path.join("instance.cfg").exists() {
        meta.importable = true;
        meta.format = Some("instance.cfg".to_string());
        return meta;
    }

    if path.join(".minecraft").is_dir() || path.join("versions").is_dir() {
        meta.importable = true;
        meta.format = Some("minecraft-directory".to_string());
    }

    meta
}

fn dir_size(path: &Path) -> u64 {
    const MAX_SIZE_SCAN_DEPTH: usize = 6;
    const MAX_SIZE_SCAN_ENTRIES: usize = 5_000;

    fn inner(path: &Path, depth: usize, scanned_entries: &mut usize) -> u64 {
        if *scanned_entries >= MAX_SIZE_SCAN_ENTRIES {
            return 0;
        }
        if path.is_file() {
            *scanned_entries += 1;
            return fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
        }
        if depth >= MAX_SIZE_SCAN_DEPTH {
            return 0;
        }

        let mut total = 0;
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                if *scanned_entries >= MAX_SIZE_SCAN_ENTRIES {
                    break;
                }
                let entry_path = entry.path();
                if entry_path.is_dir() {
                    total += inner(&entry_path, depth + 1, scanned_entries);
                } else {
                    *scanned_entries += 1;
                    total += fs::metadata(entry_path).map(|meta| meta.len()).unwrap_or(0);
                }
            }
        }
        total
    }

    let mut scanned_entries = 0usize;
    inner(path, 0, &mut scanned_entries)
}

fn detect_dir(path: &Path, launcher: &str) -> Option<DetectedInstance> {
    if !path.is_dir() || !has_required_instance_layout(path) {
        return None;
    }

    let mut meta = detect_from_manifest(path);
    if meta.loader.is_none() {
        if let Some((loader, loader_version)) = detect_loader_from_versions_dir(path) {
            meta.loader = Some(loader);
            meta.loader_version = Some(loader_version);
        }
    }
    let name = path
        .file_name()
        .map(|v| v.to_string_lossy().to_string())
        .unwrap_or_else(|| launcher.to_string());

    let icon_candidates = ["icon.png", "instance.png", ".minecraft/icon.png"];
    let icon_path = icon_candidates
        .iter()
        .map(|candidate| path.join(candidate))
        .find(|candidate| candidate.exists())
        .map(|candidate| candidate.display().to_string());

    let mods_count = [
        path.join("mods"),
        path.join(".minecraft/mods"),
        path.join("minecraft/mods"),
    ]
    .into_iter()
    .find(|mods_path| mods_path.is_dir())
    .and_then(|mods_path| fs::read_dir(mods_path).ok())
    .map(|entries| {
        entries
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("jar"))
            })
            .count() as u32
    });

    let size_mb = {
        let size_bytes = dir_size(path);
        if size_bytes == 0 {
            None
        } else {
            Some(size_bytes / 1_048_576)
        }
    };

    let last_played = fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| modified.duration_since(SystemTime::UNIX_EPOCH).ok())
        .and_then(|value| chrono::DateTime::from_timestamp(value.as_secs() as i64, 0))
        .map(|date| date.to_rfc3339());

    let importable = meta.importable;
    let canonical_path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    canonical_path.hash(&mut hasher);
    let detected_id = format!(
        "detected-{:x}-{}",
        hasher.finish(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );

    Some(DetectedInstance {
        id: detected_id,
        name,
        source_launcher: launcher.to_string(),
        source_path: path.display().to_string(),
        minecraft_version: meta
            .minecraft_version
            .unwrap_or_else(|| "desconocida".to_string()),
        loader: meta.loader.unwrap_or_else(|| "vanilla".to_string()),
        loader_version: meta.loader_version.unwrap_or_else(|| "-".to_string()),
        format: meta.format.unwrap_or_else(|| "directory".to_string()),
        icon_path,
        mods_count,
        size_mb,
        last_played,
        importable,
        import_warnings: if importable {
            Vec::new()
        } else {
            vec!["No se detectaron archivos de formato conocido".to_string()]
        },
    })
}

fn guess_icon_mime(icon_path: &Path) -> &'static str {
    match icon_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        _ => "image/png",
    }
}

fn persist_shortcut_visual_meta(instance_root: &Path, source_path: &Path) {
    let icon_candidates = ["icon.png", "instance.png", ".minecraft/icon.png"];
    let Some(icon_path) = icon_candidates
        .iter()
        .map(|candidate| source_path.join(candidate))
        .find(|candidate| candidate.exists())
    else {
        return;
    };

    let Ok(icon_bytes) = fs::read(&icon_path) else {
        return;
    };

    let mime = guess_icon_mime(&icon_path);
    let media_data_url = format!("data:{mime};base64,{}", STANDARD.encode(icon_bytes));
    let visual_meta = serde_json::json!({
        "mediaDataUrl": media_data_url,
        "mediaMime": mime,
    });
    let _ = fs::write(
        instance_root.join(".interface-visual.json"),
        serde_json::to_string_pretty(&visual_meta).unwrap_or_else(|_| "{}".to_string()),
    );
}

fn dedupe_instances(instances: Vec<DetectedInstance>) -> Vec<DetectedInstance> {
    let mut by_path = HashSet::new();
    let mut by_name = HashSet::new();
    let mut out = Vec::new();

    for instance in instances {
        let canonical_key = fs::canonicalize(&instance.source_path)
            .unwrap_or_else(|_| PathBuf::from(&instance.source_path))
            .to_string_lossy()
            .to_string();
        if !by_path.insert(canonical_key) {
            continue;
        }

        let normalized_name = instance.name.trim().to_ascii_lowercase();
        if normalized_name.is_empty() || !by_name.insert(normalized_name) {
            continue;
        }

        out.push(instance);
    }

    out
}

fn collect_candidate_instance_dirs(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![root.to_path_buf()];
    let mut queue = VecDeque::from([(root.to_path_buf(), 0usize)]);
    let mut visited = HashSet::new();

    while let Some((current, depth)) = queue.pop_front() {
        if depth >= 2 || out.len() >= MAX_ROOT_CHILDREN_TO_SCAN {
            continue;
        }

        let canonical = fs::canonicalize(&current).unwrap_or(current.clone());
        if !visited.insert(canonical) {
            continue;
        }

        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };

        for entry in entries.flatten() {
            if out.len() >= MAX_ROOT_CHILDREN_TO_SCAN {
                break;
            }
            let path = entry.path();
            if !path.is_dir() || should_skip_scan_dir(&path) {
                continue;
            }

            if has_required_instance_layout(&path) {
                out.push(path.clone());
            }

            queue.push_back((path, depth + 1));
        }
    }

    out
}

#[tauri::command]
pub fn detect_external_instances(app: AppHandle) -> Result<Vec<DetectedInstance>, String> {
    CANCEL_IMPORT
        .get_or_init(|| Arc::new(AtomicBool::new(false)))
        .store(false, Ordering::Relaxed);

    let mut found = Vec::new();
    let mut seen_paths = HashSet::new();

    let scan_targets = known_and_discovered_paths();
    let total_targets = scan_targets.len().max(1);

    for (index, (launcher, root)) in scan_targets.into_iter().enumerate() {
        let percent = (((index as f32) / (total_targets as f32)) * 100.0).round() as usize;
        let _ = app.emit(
            "import_scan_progress",
            ScanProgressEvent {
                stage: format!("scanning_{}", launcher.to_lowercase().replace(' ', "_")),
                message: format!("Buscando en {launcher}..."),
                found_so_far: found.len(),
                current_path: root.display().to_string(),
                progress_percent: percent.min(100) as u8,
                total_targets,
            },
        );

        if !root.exists() || !root.is_dir() {
            continue;
        }

        let canonical = fs::canonicalize(&root).unwrap_or(root.clone());
        if seen_paths.insert(canonical) {
            if let Some(instance) = detect_dir(&root, &launcher) {
                let _ = app.emit("import_scan_result", instance.clone());
                found.push(instance);
            }
        }

        let candidates = collect_candidate_instance_dirs(&root);

        for (candidate_index, path) in candidates.into_iter().enumerate() {
            if CANCEL_IMPORT
                .get()
                .is_some_and(|flag| flag.load(Ordering::Relaxed))
            {
                return Ok(found);
            }

            if candidate_index % SCAN_PROGRESS_EMIT_INTERVAL == 0 {
                let _ = app.emit(
                    "import_scan_progress",
                    ScanProgressEvent {
                        stage: format!("scanning_{}", launcher.to_lowercase().replace(' ', "_")),
                        message: "Escaneando carpeta...".to_string(),
                        found_so_far: found.len(),
                        current_path: path.display().to_string(),
                        progress_percent: percent.min(99) as u8,
                        total_targets,
                    },
                );
            }

            let canonical = fs::canonicalize(&path).unwrap_or(path.clone());
            if !seen_paths.insert(canonical) {
                continue;
            }

            if let Some(instance) = detect_dir(&path, &launcher) {
                let _ = app.emit("import_scan_result", instance.clone());
                found.push(instance);
            }
        }
    }

    let _ = app.emit(
        "import_scan_progress",
        ScanProgressEvent {
            stage: "completed".to_string(),
            message: "Escaneo completado".to_string(),
            found_so_far: found.len(),
            current_path: String::new(),
            progress_percent: 100,
            total_targets,
        },
    );

    let found = dedupe_instances(found);

    Ok(found)
}

#[tauri::command]
pub fn import_specific(path: String) -> Result<Vec<DetectedInstance>, String> {
    let p = PathBuf::from(path);
    if p.is_file() {
        let name = p
            .file_name()
            .map(|v| v.to_string_lossy().to_string())
            .unwrap_or_else(|| "archivo".to_string());
        return Ok(vec![DetectedInstance {
            id: Uuid::new_v4().to_string(),
            name,
            source_launcher: "Archivo".to_string(),
            source_path: p.display().to_string(),
            minecraft_version: "desconocida".to_string(),
            loader: "desconocido".to_string(),
            loader_version: "-".to_string(),
            format: p
                .extension()
                .map(|v| v.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            icon_path: None,
            mods_count: None,
            size_mb: None,
            last_played: None,
            importable: true,
            import_warnings: vec![],
        }]);
    }

    if p.is_dir() {
        if let Some(main) = detect_dir(&p, "Manual") {
            if main.importable {
                return Ok(vec![main]);
            }
        }

        let mut out = Vec::new();
        for entry in
            fs::read_dir(&p).map_err(|e| format!("No se pudo leer {}: {e}", p.display()))?
        {
            let entry = entry.map_err(|e| format!("No se pudo leer entrada: {e}"))?;
            if let Some(instance) = detect_dir(&entry.path(), "Manual") {
                out.push(instance);
            }
        }
        return Ok(dedupe_instances(out));
    }

    Ok(Vec::new())
}

#[tauri::command]
pub fn execute_import(app: AppHandle, requests: Vec<ImportRequest>) -> Result<(), String> {
    use crate::app::settings_service::resolve_instances_root;

    let instances_root = resolve_instances_root(&app)?;
    fs::create_dir_all(&instances_root)
        .map_err(|err| format!("No se pudo preparar el directorio de instancias: {err}"))?;

    for (index, req) in requests.iter().enumerate() {
        let source_root = PathBuf::from(&req.source_path);
        if !source_root.exists() || !source_root.is_dir() {
            let _ = app.emit(
                "import_instance_completed",
                serde_json::json!({
                    "success": false,
                    "instanceId": req.detected_instance_id,
                    "error": format!("Ruta inválida: {}", source_root.display())
                }),
            );
            continue;
        }

        let mut sanitized_name = sanitize_path_segment(&req.target_name);
        if sanitized_name.trim().is_empty() {
            sanitized_name = format!("imported-{}", index + 1);
        }

        let mut instance_root = instances_root.join(&sanitized_name);
        if instance_root.exists() {
            let suffix = uuid::Uuid::new_v4().simple().to_string();
            instance_root = instances_root.join(format!("{}-{}", sanitized_name, &suffix[..8]));
        }

        let _ = app.emit(
            "import_execution_progress",
            serde_json::json!({
                "instanceId": req.detected_instance_id,
                "instanceName": req.target_name,
                "step": "creating_instance",
                "stepIndex": 1,
                "totalSteps": 3,
                "completed": index,
                "total": requests.len(),
                "message": format!("Creando {}", req.target_name)
            }),
        );

        let result = (|| -> Result<(), String> {
            fs::create_dir_all(&instance_root).map_err(|err| {
                format!(
                    "No se pudo crear la instancia {}: {err}",
                    instance_root.display()
                )
            })?;

            let mut copied_files = 0usize;
            copy_dir_recursive_limited(&source_root, &instance_root, &mut copied_files, None)?;

            let effective_version_id = resolve_effective_version_id(
                &source_root,
                &req.minecraft_version,
                &req.loader,
                &req.loader_version,
                "Auto detectado",
            );

            let internal_uuid = uuid::Uuid::new_v4().to_string();
            let mut metadata = InstanceMetadata {
                name: req.target_name.clone(),
                group: req.target_group.clone(),
                minecraft_version: req.minecraft_version.clone(),
                version_id: effective_version_id,
                loader: req.loader.clone(),
                loader_version: req.loader_version.clone(),
                ram_mb: req.ram_mb,
                java_args: vec!["-XX:+UnlockExperimentalVMOptions".to_string()],
                java_path: "".to_string(),
                java_runtime: "imported".to_string(),
                java_version: "".to_string(),
                required_java_major: 0,
                created_at: chrono::Utc::now().to_rfc3339(),
                state: "IMPORTED".to_string(),
                last_used: None,
                internal_uuid,
            };

            finalize_import_runtime(&app, &instance_root, &source_root, &mut metadata)?;

            let metadata_path = instance_root.join(".instance.json");
            let metadata_raw = serde_json::to_string_pretty(&metadata)
                .map_err(|err| format!("No se pudo serializar metadata: {err}"))?;
            fs::write(&metadata_path, metadata_raw)
                .map_err(|err| format!("No se pudo guardar metadata: {err}"))?;

            Ok(())
        })();

        match result {
            Ok(()) => {
                let _ = app.emit(
                    "import_instance_completed",
                    serde_json::json!({
                        "success": true,
                        "instanceId": req.detected_instance_id,
                        "error": serde_json::Value::Null
                    }),
                );
            }
            Err(error) => {
                let _ = app.emit(
                    "import_instance_completed",
                    serde_json::json!({
                        "success": false,
                        "instanceId": req.detected_instance_id,
                        "error": error
                    }),
                );
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub fn execute_import_action(
    app: AppHandle,
    request: ImportActionRequest,
) -> Result<ImportActionResult, String> {
    let action = request.action.trim().to_ascii_lowercase();

    if action == "abrir_carpeta" {
        crate::app::instance_service::open_instance_folder(request.source_path.clone())?;
        return Ok(ImportActionResult {
            success: true,
            target_name: request.target_name,
            target_path: Some(request.source_path),
            error: None,
        });
    }

    if action == "eliminar_instancia" {
        let source_path = PathBuf::from(&request.source_path);
        if !source_path.exists() {
            return Err(format!(
                "La instancia ya no existe en {}",
                source_path.display()
            ));
        }
        if !source_path.is_dir() {
            return Err(format!(
                "La ruta no es una carpeta válida: {}",
                source_path.display()
            ));
        }
        fs::remove_dir_all(&source_path)
            .map_err(|err| format!("No se pudo eliminar la instancia origen: {err}"))?;
        return Ok(ImportActionResult {
            success: true,
            target_name: request.target_name,
            target_path: Some(request.source_path),
            error: None,
        });
    }

    if action == "ejecutar" {
        let source_path = PathBuf::from(&request.source_path);
        if !source_path.exists() {
            return Err(format!("La carpeta original de la instancia ya no existe en: {}. Es posible que el launcher externo haya movido o eliminado la instancia.", source_path.display()));
        }
        if !source_path.is_dir() {
            return Err(format!(
                "La ruta de instancia original no es una carpeta válida: {}",
                source_path.display()
            ));
        }

        let hints = crate::app::redirect_launch::RedirectVersionHints {
            minecraft_version: request.minecraft_version.clone(),
            loader: request.loader.clone(),
            loader_version: request.loader_version.clone(),
        };
        let effective_version_id = resolve_effective_version_id(
            &source_path,
            &request.minecraft_version,
            &request.loader,
            &request.loader_version,
            &request.source_launcher,
        );

        crate::app::redirect_launch::resolve_redirect_launch_context(
            &source_path,
            &effective_version_id,
            &request.source_launcher,
            &hints,
        )?;

        let cache_uuid = format!(
            "preview-{:x}",
            md5::compute(format!("{}::{effective_version_id}", source_path.display()))
        );
        let _ =
            tauri::async_runtime::block_on(crate::app::redirect_launch::prewarm_redirect_runtime(
                &app,
                &source_path,
                &request.source_launcher,
                &cache_uuid,
                &effective_version_id,
                &hints,
            ));

        return Ok(ImportActionResult {
            success: true,
            target_name: request.target_name,
            target_path: Some(request.source_path),
            error: None,
        });
    }

    if action == "crear_atajo" {
        let source_root = PathBuf::from(&request.source_path);
        if !source_root.exists() {
            return Err(format!(
                "No se puede crear el atajo porque la instancia origen no existe: {}",
                source_root.display()
            ));
        }
        if !source_root.is_dir() {
            return Err(format!(
                "No se puede crear el atajo porque el origen no es una carpeta válida: {}",
                source_root.display()
            ));
        }

        let instances_root = crate::app::settings_service::resolve_instances_root(&app)?;
        fs::create_dir_all(&instances_root)
            .map_err(|err| format!("No se pudo preparar directorio de instancias: {err}"))?;

        let mut sanitized_name = sanitize_path_segment(&request.target_name);
        if sanitized_name.trim().is_empty() {
            sanitized_name = "instancia-atajo".to_string();
        }

        let mut instance_root = instances_root.join(&sanitized_name);
        let mut suffix = 1u32;
        while instance_root.exists() {
            instance_root = instances_root.join(format!("{}-atajo-{}", sanitized_name, suffix));
            suffix += 1;
        }

        fs::create_dir_all(&instance_root)
            .map_err(|err| format!("No se pudo crear carpeta del atajo: {err}"))?;

        let mut shortcut_loader = request.loader.clone();
        let mut shortcut_loader_version = request.loader_version.clone();
        let requested_loader_normalized = normalize_loader(&shortcut_loader);
        let request_is_vanilla = matches!(
            requested_loader_normalized.as_str(),
            "" | "-" | "vanilla" | "desconocido" | "unknown"
        );
        if let Some((detected_loader, detected_loader_version)) =
            detect_loader_from_versions_dir(&source_root)
        {
            let detected_is_vanilla = matches!(
                normalize_loader(&detected_loader).as_str(),
                "" | "-" | "vanilla" | "desconocido" | "unknown"
            );
            if is_unknown_loader(&shortcut_loader) || (request_is_vanilla && !detected_is_vanilla) {
                shortcut_loader = detected_loader;
            }
            if shortcut_loader_version.trim().is_empty()
                || shortcut_loader_version == "-"
                || (request_is_vanilla && !detected_loader_version.trim().is_empty())
            {
                shortcut_loader_version = detected_loader_version;
            }
        }

        let mut effective_version_id = resolve_effective_version_id(
            &source_root,
            &request.minecraft_version,
            &shortcut_loader,
            &shortcut_loader_version,
            &request.source_launcher,
        );
        let loader_is_vanilla = matches!(
            shortcut_loader.trim().to_ascii_lowercase().as_str(),
            "" | "-" | "vanilla" | "desconocido" | "unknown"
        );
        if !loader_is_vanilla
            && !version_id_contains_loader(&effective_version_id, &shortcut_loader)
        {
            if let Some(discovered_version_id) = find_loader_version_id_from_external_paths(
                &source_root,
                &request.source_launcher,
                &request.minecraft_version,
                &shortcut_loader,
            ) {
                log::info!(
                    "[REDIRECT] version_id de loader resuelto desde rutas externas: {}",
                    discovered_version_id
                );
                effective_version_id = discovered_version_id;
            }
        }

        if let Some((detected_loader, detected_loader_version)) =
            detect_loader_from_version_id(&effective_version_id)
        {
            if is_unknown_loader(&shortcut_loader) {
                shortcut_loader = detected_loader;
            }
            if shortcut_loader_version.is_empty() || shortcut_loader_version == "-" {
                shortcut_loader_version = detected_loader_version;
                log::info!(
                    "[REDIRECT] loader_version detectado desde version_id: {}",
                    shortcut_loader_version
                );
            }
        }

        let mut metadata = InstanceMetadata {
            name: request.target_name.clone(),
            group: "Atajos".to_string(),
            minecraft_version: request.minecraft_version.clone(),
            version_id: effective_version_id,
            loader: shortcut_loader,
            loader_version: shortcut_loader_version,
            ram_mb: 4096,
            java_args: vec!["-XX:+UnlockExperimentalVMOptions".to_string()],
            java_path: "".to_string(),
            java_runtime: "shortcut".to_string(),
            java_version: "".to_string(),
            required_java_major: 0,
            created_at: chrono::Utc::now().to_rfc3339(),
            state: "REDIRECT".to_string(),
            last_used: None,
            internal_uuid: uuid::Uuid::new_v4().to_string(),
        };

        let metadata_path = instance_root.join(".instance.json");
        let metadata_raw = serde_json::to_string_pretty(&metadata)
            .map_err(|err| format!("No se pudo serializar metadata de atajo: {err}"))?;
        fs::write(&metadata_path, metadata_raw)
            .map_err(|err| format!("No se pudo guardar metadata de atajo: {err}"))?;

        let redirect = ShortcutRedirect {
            source_path: request.source_path.clone(),
            source_launcher: request.source_launcher.clone(),
        };
        let redirect_path = instance_root.join(".redirect.json");
        let redirect_raw = serde_json::to_string_pretty(&redirect)
            .map_err(|err| format!("No se pudo serializar redirección de atajo: {err}"))?;
        fs::write(&redirect_path, redirect_raw)
            .map_err(|err| format!("No se pudo guardar redirección de atajo: {err}"))?;

        persist_shortcut_visual_meta(&instance_root, Path::new(&request.source_path));

        let source_path = PathBuf::from(&request.source_path);
        let hints = crate::app::redirect_launch::RedirectVersionHints {
            minecraft_version: metadata.minecraft_version.clone(),
            loader: metadata.loader.clone(),
            loader_version: metadata.loader_version.clone(),
        };

        let mut prewarm_candidates = Vec::new();
        prewarm_candidates.push(metadata.version_id.clone());
        prewarm_candidates.push(resolve_shortcut_version_id(
            &metadata.minecraft_version,
            &metadata.loader,
            &metadata.loader_version,
        ));
        prewarm_candidates.push(metadata.minecraft_version.clone());

        let mut seen_candidates = HashSet::new();
        prewarm_candidates.retain(|candidate| {
            let key = candidate.trim().to_ascii_lowercase();
            !key.is_empty() && seen_candidates.insert(key)
        });

        let mut prewarm_errors = Vec::new();
        let mut selected_runtime_version: Option<String> = None;
        for candidate in prewarm_candidates {
            match tauri::async_runtime::block_on(
                crate::app::redirect_launch::prewarm_redirect_runtime(
                    &app,
                    &source_path,
                    &request.source_launcher,
                    &metadata.internal_uuid,
                    &candidate,
                    &hints,
                ),
            ) {
                Ok(()) => {
                    selected_runtime_version = Some(candidate);
                    break;
                }
                Err(err) => {
                    prewarm_errors.push(format!("{candidate}: {err}"));
                }
            }
        }

        if let Some(runtime_version) = selected_runtime_version {
            if runtime_version != metadata.version_id {
                metadata.version_id = runtime_version;
                let metadata_raw = serde_json::to_string_pretty(&metadata)
                    .map_err(|err| format!("No se pudo serializar metadata de atajo: {err}"))?;
                fs::write(&metadata_path, metadata_raw)
                    .map_err(|err| format!("No se pudo guardar metadata de atajo: {err}"))?;
            }
        } else {
            let warning = if prewarm_errors.is_empty() {
                "No se pudo validar runtime REDIRECT durante la creación; se reintentará al iniciar la instancia.".to_string()
            } else {
                format!(
                    "No se pudo completar prewarm REDIRECT en la creación; la instancia fue creada y se reintentará al iniciar. Detalle: {}",
                    prewarm_errors.join(" | ")
                )
            };
            log::warn!("[REDIRECT] {}", warning);
            let _ = app.emit(
                "import_execution_progress",
                serde_json::json!({
                    "instanceId": request.detected_instance_id,
                    "instanceName": request.target_name,
                    "action": "crear_atajo",
                    "step": "runtime_warning",
                    "message": warning,
                    "stepIndex": 2,
                    "totalSteps": 3,
                }),
            );
        }

        let _ = app.emit(
            "instances_changed",
            serde_json::json!({
                "action": "created",
                "instanceName": request.target_name,
                "instancePath": instance_root.display().to_string(),
            }),
        );

        return Ok(ImportActionResult {
            success: true,
            target_name: request.target_name,
            target_path: Some(instance_root.display().to_string()),
            error: None,
        });
    }

    let import_request = ImportRequest {
        detected_instance_id: request.detected_instance_id,
        source_path: request.source_path.clone(),
        target_name: request.target_name.clone(),
        target_group: request.target_group,
        minecraft_version: request.minecraft_version,
        loader: request.loader,
        loader_version: request.loader_version,
        ram_mb: 4096,
        copy_mods: true,
        copy_worlds: true,
        copy_resourcepacks: true,
        copy_screenshots: true,
        copy_logs: true,
    };

    execute_import(app.clone(), vec![import_request])?;

    if action == "migrar" {
        let source_path = PathBuf::from(&request.source_path);
        if source_path.exists() && source_path.is_dir() {
            fs::remove_dir_all(&source_path).map_err(|err| {
                format!("No se pudo eliminar la instancia original tras migrar: {err}")
            })?;
        }
    }

    Ok(ImportActionResult {
        success: true,
        target_name: request.target_name,
        target_path: None,
        error: None,
    })
}

#[tauri::command]
pub fn execute_import_action_batch(
    app: AppHandle,
    action: String,
    requests: Vec<ImportActionRequest>,
) -> Result<ImportActionBatchResult, String> {
    let normalized_action = action.trim().to_ascii_lowercase();
    let total = requests.len();
    let mut failures = Vec::new();
    let mut success_count = 0usize;

    for (index, mut request) in requests.into_iter().enumerate() {
        request.action = normalized_action.clone();
        let instance_id = request.detected_instance_id.clone();
        let target_name = request.target_name.clone();

        let should_emit_focus = matches!(
            normalized_action.as_str(),
            "crear_atajo" | "clonar" | "migrar"
        );
        if should_emit_focus {
            emit_action_progress(
                &app,
                &request,
                &normalized_action,
                index,
                total,
                0,
                3,
                "verificando",
                format!(
                    "Primer foco: verificador analizando runtime y loader para {}...",
                    request.target_name
                ),
                Some(vec![
                    ImportFocusStatus {
                        key: "verifier".to_string(),
                        label: "Primer foco · Verificador".to_string(),
                        status: "running".to_string(),
                    },
                    ImportFocusStatus {
                        key: "downloader".to_string(),
                        label: "Segundo foco · Descargador e instalador".to_string(),
                        status: "idle".to_string(),
                    },
                    ImportFocusStatus {
                        key: "finalizer".to_string(),
                        label: "Tercer foco · Finalización".to_string(),
                        status: "idle".to_string(),
                    },
                ]),
            );
            emit_action_progress(
                &app,
                &request,
                &normalized_action,
                index,
                total,
                1,
                3,
                "descargando",
                "Segundo foco: descargando e instalando componentes faltantes oficiales..."
                    .to_string(),
                Some(vec![
                    ImportFocusStatus {
                        key: "verifier".to_string(),
                        label: "Primer foco · Verificador".to_string(),
                        status: "ok".to_string(),
                    },
                    ImportFocusStatus {
                        key: "downloader".to_string(),
                        label: "Segundo foco · Descargador e instalador".to_string(),
                        status: "running".to_string(),
                    },
                    ImportFocusStatus {
                        key: "finalizer".to_string(),
                        label: "Tercer foco · Finalización".to_string(),
                        status: "idle".to_string(),
                    },
                ]),
            );
        }

        let result = execute_import_action(app.clone(), request);

        match result {
            Ok(response) if response.success => {
                success_count += 1;
                if should_emit_focus {
                    let done = index + 1;
                    emit_action_progress(
                        &app,
                        &ImportActionRequest {
                            detected_instance_id: instance_id.clone(),
                            source_path: String::new(),
                            target_name: response.target_name.clone(),
                            target_group: String::new(),
                            minecraft_version: String::new(),
                            loader: String::new(),
                            loader_version: String::new(),
                            source_launcher: String::new(),
                            action: normalized_action.clone(),
                        },
                        &normalized_action,
                        done,
                        total,
                        3,
                        3,
                        "finalizado",
                        format!(
                            "Tercer foco: proceso completado para {}.",
                            response.target_name
                        ),
                        Some(vec![
                            ImportFocusStatus {
                                key: "verifier".to_string(),
                                label: "Primer foco · Verificador".to_string(),
                                status: "ok".to_string(),
                            },
                            ImportFocusStatus {
                                key: "downloader".to_string(),
                                label: "Segundo foco · Descargador e instalador".to_string(),
                                status: "ok".to_string(),
                            },
                            ImportFocusStatus {
                                key: "finalizer".to_string(),
                                label: "Tercer foco · Finalización".to_string(),
                                status: "ok".to_string(),
                            },
                        ]),
                    );
                }
            }
            Ok(response) => {
                failures.push(ImportActionBatchFailure {
                    instance_id: instance_id.clone(),
                    target_name: response.target_name,
                    error: response
                        .error
                        .unwrap_or_else(|| "La acción terminó sin éxito".to_string()),
                });
                if should_emit_focus {
                    emit_action_progress(
                        &app,
                        &ImportActionRequest {
                            detected_instance_id: instance_id.clone(),
                            source_path: String::new(),
                            target_name: target_name.clone(),
                            target_group: String::new(),
                            minecraft_version: String::new(),
                            loader: String::new(),
                            loader_version: String::new(),
                            source_launcher: String::new(),
                            action: normalized_action.clone(),
                        },
                        &normalized_action,
                        index,
                        total,
                        2,
                        3,
                        "error",
                        format!(
                            "Tercer foco: no se pudo completar {}, se cancela para evitar errores.",
                            target_name
                        ),
                        Some(vec![
                            ImportFocusStatus {
                                key: "verifier".to_string(),
                                label: "Primer foco · Verificador".to_string(),
                                status: "warn".to_string(),
                            },
                            ImportFocusStatus {
                                key: "downloader".to_string(),
                                label: "Segundo foco · Descargador e instalador".to_string(),
                                status: "error".to_string(),
                            },
                            ImportFocusStatus {
                                key: "finalizer".to_string(),
                                label: "Tercer foco · Finalización".to_string(),
                                status: "error".to_string(),
                            },
                        ]),
                    );
                }
            }
            Err(error) => {
                failures.push(ImportActionBatchFailure {
                    instance_id: instance_id.clone(),
                    target_name: target_name.clone(),
                    error,
                });
                if should_emit_focus {
                    emit_action_progress(
                        &app,
                        &ImportActionRequest {
                            detected_instance_id: instance_id,
                            source_path: String::new(),
                            target_name,
                            target_group: String::new(),
                            minecraft_version: String::new(),
                            loader: String::new(),
                            loader_version: String::new(),
                            source_launcher: String::new(),
                            action: normalized_action.clone(),
                        },
                        &normalized_action,
                        index,
                        total,
                        2,
                        3,
                        "error",
                        "Tercer foco: error crítico, operación cancelada para prevenir problemas."
                            .to_string(),
                        Some(vec![
                            ImportFocusStatus {
                                key: "verifier".to_string(),
                                label: "Primer foco · Verificador".to_string(),
                                status: "warn".to_string(),
                            },
                            ImportFocusStatus {
                                key: "downloader".to_string(),
                                label: "Segundo foco · Descargador e instalador".to_string(),
                                status: "error".to_string(),
                            },
                            ImportFocusStatus {
                                key: "finalizer".to_string(),
                                label: "Tercer foco · Finalización".to_string(),
                                status: "error".to_string(),
                            },
                        ]),
                    );
                }
            }
        }
    }

    Ok(ImportActionBatchResult {
        success: failures.is_empty(),
        action: normalized_action,
        total,
        success_count,
        failure_count: failures.len(),
        failures,
    })
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<(), String> {
    if !source.exists() {
        return Err(format!(
            "La instancia origen no existe: {}",
            source.display()
        ));
    }
    for entry in fs::read_dir(source)
        .map_err(|err| format!("No se pudo leer {}: {err}", source.display()))?
    {
        let entry = entry
            .map_err(|err| format!("No se pudo leer entrada de {}: {err}", source.display()))?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        if from.is_dir() {
            fs::create_dir_all(&to)
                .map_err(|err| format!("No se pudo crear {}: {err}", to.display()))?;
            copy_dir_recursive(&from, &to)?;
        } else {
            fs::copy(&from, &to).map_err(|err| {
                format!(
                    "No se pudo copiar {} -> {}: {err}",
                    from.display(),
                    to.display()
                )
            })?;
        }
    }
    Ok(())
}

#[tauri::command]
pub fn cancel_import() {
    if let Some(flag) = CANCEL_IMPORT.get() {
        flag.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::{detect_loader_from_versions_dir, has_required_instance_layout};
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_dir(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("interface-import-{label}-{stamp}"));
        fs::create_dir_all(&path).expect("create temp");
        path
    }

    #[test]
    fn detect_loader_in_minecraft_versions_subdir() {
        let root = temp_dir("loader-minecraft-subdir");
        let version_id = "fabric-loader-0.16.9-1.20.1";
        let version_dir = root.join("minecraft/versions").join(version_id);
        fs::create_dir_all(&version_dir).expect("create versions");

        let detected = detect_loader_from_versions_dir(&root);
        fs::remove_dir_all(&root).ok();

        assert_eq!(detected, Some(("fabric".to_string(), "0.16.9".to_string())));
    }

    #[test]
    fn reject_global_minecraft_directory_as_instance() {
        let root = temp_dir("global-minecraft").join(".minecraft");
        let versions = root.join("versions/1.20.1");
        fs::create_dir_all(&versions).expect("create version root");
        fs::write(versions.join("1.20.1.json"), "{}").expect("json");
        fs::write(versions.join("1.20.1.jar"), "jar").expect("jar");
        fs::create_dir_all(root.join("assets")).expect("assets");
        fs::create_dir_all(root.join("libraries")).expect("libraries");
        fs::write(root.join("options.txt"), "").expect("options");

        let is_instance = has_required_instance_layout(&root);
        let cleanup = root.parent().expect("temp parent").to_path_buf();
        fs::remove_dir_all(cleanup).ok();

        assert!(!is_instance);
    }
}
