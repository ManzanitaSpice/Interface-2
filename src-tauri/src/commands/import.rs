use std::{
    collections::{HashSet, VecDeque},
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, OnceLock,
    },
    time::SystemTime,
};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

use crate::{
    domain::models::instance::InstanceMetadata,
    infrastructure::filesystem::paths::sanitize_path_segment,
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

static CANCEL_IMPORT: OnceLock<Arc<AtomicBool>> = OnceLock::new();

const INSTANCE_IDENTIFIER_FILES: &[&str] = &[
    "minecraftinstance.json",
    "mmc-pack.json",
    "profile.json",
    "instance.cfg",
    ".curseclient",
];

const INSTANCE_MINECRAFT_DIRS: &[&str] = &[".minecraft", "minecraft"];
const INSTANCE_HINT_KEYWORDS: &[&str] = &[
    "instancias",
    "instances",
    "instance",
    "launcher",
    "minecraft",
    "modpacks",
    "prism",
    "multimc",
    "curseforge",
    "curse",
    "mmc",
];

const SCAN_SKIP_DIR_NAMES: &[&str] = &[
    "node_modules",
    "target",
    ".git",
    ".cache",
    ".cargo",
    ".rustup",
    "Library",
    "AppData",
    "Program Files",
    "Program Files (x86)",
    "Windows",
    "System Volume Information",
];

fn known_paths() -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();

    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            out.push((
                "CurseForge".to_string(),
                PathBuf::from(&appdata).join("CurseForge/Minecraft/Instances"),
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
        }
    }

    out
}

fn has_instance_markers(path: &Path) -> bool {
    INSTANCE_IDENTIFIER_FILES
        .iter()
        .any(|file| path.join(file).is_file())
}

fn has_required_instance_layout(path: &Path) -> bool {
    let has_minecraft_folder = INSTANCE_MINECRAFT_DIRS
        .iter()
        .any(|dir| path.join(dir).is_dir());
    let has_identifier = has_instance_markers(path);
    let has_common_markers = path.join("mods").is_dir()
        || path.join("versions").is_dir()
        || path.join("options.txt").is_file()
        || path.join(".minecraft/mods").is_dir();

    has_identifier || (has_minecraft_folder && has_common_markers)
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
        for drive in b'A'..=b'Z' {
            let root = PathBuf::from(format!("{}:\\", drive as char));
            if root.exists() {
                roots.push(root);
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        for candidate in ["/media", "/mnt", "/run/media", "/Volumes"] {
            let path = PathBuf::from(candidate);
            if path.exists() {
                roots.push(path);
            }
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        let home = PathBuf::from(home);
        roots.push(home.join("Desktop"));
        roots.push(home.join("Documents"));
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
        if found.len() >= max_candidates {
            break;
        }

        let canonical = fs::canonicalize(&current).unwrap_or(current.clone());
        if !visited.insert(canonical) {
            continue;
        }

        if directory_name_looks_like_container(&current) || has_instance_markers(&current) {
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

        for discovered in discover_keyword_scan_paths(&base, 3, 40) {
            let canonical = fs::canonicalize(&discovered).unwrap_or(discovered.clone());
            if seen.insert(canonical) {
                out.push(("Auto detectado".to_string(), discovered));
            }
        }
    }

    out
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
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
    let versions_candidates = [path.join("versions"), path.join(".minecraft/versions")];
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
    Some(DetectedInstance {
        id: Uuid::new_v4().to_string(),
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
    let mut by_signature = HashSet::new();
    let mut out = Vec::new();

    for instance in instances {
        let canonical_key = fs::canonicalize(&instance.source_path)
            .unwrap_or_else(|_| PathBuf::from(&instance.source_path))
            .to_string_lossy()
            .to_string();
        if !by_path.insert(canonical_key) {
            continue;
        }

        let signature = format!(
            "{}::{}::{}",
            instance.name.trim().to_ascii_lowercase(),
            instance.minecraft_version.trim().to_ascii_lowercase(),
            instance.loader.trim().to_ascii_lowercase()
        );

        if !by_signature.insert(signature) {
            continue;
        }

        out.push(instance);
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

        let entries = match fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            if CANCEL_IMPORT
                .get()
                .is_some_and(|flag| flag.load(Ordering::Relaxed))
            {
                return Ok(found);
            }

            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            if should_skip_scan_dir(&path) {
                continue;
            }

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

    fn copy_recursive_limited(
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
                copy_recursive_limited(&path, &target, copied, max_files)?;
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

        let minecraft_root = instance_root.join("minecraft");
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
            fs::create_dir_all(&minecraft_root).map_err(|err| {
                format!(
                    "No se pudo crear la instancia {}: {err}",
                    instance_root.display()
                )
            })?;

            let source_minecraft_root = resolve_source_minecraft_root(&source_root);

            let mut copied_files = 0usize;
            copy_recursive_limited(
                &source_minecraft_root,
                &minecraft_root,
                &mut copied_files,
                None,
            )?;

            if req.copy_mods {
                copy_recursive_limited(
                    &source_minecraft_root.join("mods"),
                    &minecraft_root.join("mods"),
                    &mut copied_files,
                    None,
                )?;
            }
            if req.copy_worlds {
                copy_recursive_limited(
                    &source_minecraft_root.join("saves"),
                    &minecraft_root.join("saves"),
                    &mut copied_files,
                    None,
                )?;
            }
            if req.copy_resourcepacks {
                copy_recursive_limited(
                    &source_minecraft_root.join("resourcepacks"),
                    &minecraft_root.join("resourcepacks"),
                    &mut copied_files,
                    None,
                )?;
            }
            if req.copy_screenshots {
                copy_recursive_limited(
                    &source_minecraft_root.join("screenshots"),
                    &minecraft_root.join("screenshots"),
                    &mut copied_files,
                    None,
                )?;
            }
            if req.copy_logs {
                copy_recursive_limited(
                    &source_minecraft_root.join("logs"),
                    &minecraft_root.join("logs"),
                    &mut copied_files,
                    None,
                )?;
            }

            let internal_uuid = uuid::Uuid::new_v4().to_string();
            let metadata = InstanceMetadata {
                name: req.target_name.clone(),
                group: req.target_group.clone(),
                minecraft_version: req.minecraft_version.clone(),
                version_id: req.minecraft_version.clone(),
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

    if action == "ejecutar" {
        let source_path = PathBuf::from(&request.source_path);
        if !source_path.exists() {
            return Err(format!("La carpeta original de la instancia ya no existe en: {}. Es posible que el launcher externo haya movido o eliminado la instancia.", source_path.display()));
        }

        crate::app::redirect_launch::resolve_redirect_launch_context(
            &source_path,
            &request.minecraft_version,
            &request.source_launcher,
        )?;

        return Ok(ImportActionResult {
            success: true,
            target_name: request.target_name,
            target_path: Some(request.source_path),
            error: None,
        });
    }

    if action == "crear_atajo" {
        let instances_root = crate::app::settings_service::resolve_instances_root(&app)?;
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

        let metadata = InstanceMetadata {
            name: request.target_name.clone(),
            group: "Atajos".to_string(),
            minecraft_version: request.minecraft_version.clone(),
            version_id: request.minecraft_version.clone(),
            loader: request.loader.clone(),
            loader_version: request.loader_version.clone(),
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

    for mut request in requests {
        request.action = normalized_action.clone();
        let instance_id = request.detected_instance_id.clone();
        let target_name = request.target_name.clone();
        let result = execute_import_action(app.clone(), request);

        match result {
            Ok(response) if response.success => {
                success_count += 1;
            }
            Ok(response) => {
                failures.push(ImportActionBatchFailure {
                    instance_id,
                    target_name: response.target_name,
                    error: response
                        .error
                        .unwrap_or_else(|| "La acción terminó sin éxito".to_string()),
                });
            }
            Err(error) => {
                failures.push(ImportActionBatchFailure {
                    instance_id,
                    target_name,
                    error,
                });
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
fn resolve_source_minecraft_root(source_root: &Path) -> PathBuf {
    if source_root.join(".minecraft").is_dir() {
        return source_root.join(".minecraft");
    }
    if source_root.join("minecraft").is_dir() {
        return source_root.join("minecraft");
    }
    source_root.to_path_buf()
}
