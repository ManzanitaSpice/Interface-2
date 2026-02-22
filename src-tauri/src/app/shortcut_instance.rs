use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha1::{Digest, Sha1};
use tauri::AppHandle;

use crate::{
    app::redirect_launch::{build_classpath_multi, prepare_redirect_natives},
    domain::{
        java::java_requirement::determine_required_java,
        minecraft::{
            argument_resolver::{resolve_launch_arguments, LaunchContext},
            rule_engine::RuleContext,
        },
        models::{instance::LaunchAuthSession, java::JavaRuntime},
    },
    infrastructure::filesystem::paths::resolve_launcher_root,
    services::{
        instance_builder::build_instance_structure, java_installer::ensure_embedded_java,
        loader_installer::install_loader_if_needed,
    },
};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutLaunchPlan {
    pub java_path: String,
    pub main_class: String,
    #[serde(default)]
    pub jvm_args: Vec<String>,
    #[serde(default)]
    pub game_args: Vec<String>,
    #[serde(default)]
    pub classpath: Vec<String>,
    pub assets_root: String,
    pub asset_index: String,
    pub natives_dir: String,
    pub libraries_root: String,
    pub versions_root: String,
    pub version_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Signature {
    pub has_minecraftinstance_json: bool,
    pub has_pack_mcmeta: bool,
    pub has_options_txt: bool,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalLocator {
    pub last_known_path: String,
    pub signature: Signature,
    pub hints: Vec<String>,
    pub scan_roots: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutState {
    pub id: String,
    pub name: String,
    pub external_game_dir: String,
    pub external_root_dir: String,
    pub mc_version: String,
    pub loader: String,
    pub loader_version: String,
    #[serde(default = "default_creating")]
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub adopt_mode: String,
    pub locator: ExternalLocator,
    #[serde(default)]
    pub launch_plan: ShortcutLaunchPlan,
}
fn default_creating() -> String {
    "CREATING".to_string()
}

#[derive(Debug, Clone)]
pub struct ShortcutCreateRequest {
    pub name: String,
    pub target_group: String,
    pub source_launcher: String,
    pub selected_path: PathBuf,
    pub fallback_mc: String,
    pub fallback_loader: String,
    pub fallback_loader_version: String,
}

pub struct ShortcutCreateResult {
    pub instance_root: PathBuf,
}

pub fn normalize_external_dirs(user_selected_path: &Path) -> (PathBuf, PathBuf) {
    let normalized = user_selected_path.to_path_buf();
    if normalized
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case("minecraft"))
    {
        return (
            normalized.clone(),
            normalized.parent().unwrap_or(&normalized).to_path_buf(),
        );
    }
    if normalized.join("minecraft").is_dir() {
        return (normalized.join("minecraft"), normalized);
    }
    if normalized.join("mods").is_dir()
        || normalized.join("config").is_dir()
        || normalized.join("saves").is_dir()
    {
        return (
            normalized.clone(),
            normalized.parent().unwrap_or(&normalized).to_path_buf(),
        );
    }
    (
        normalized.clone(),
        normalized.parent().unwrap_or(&normalized).to_path_buf(),
    )
}

pub fn create_shortcut_instance(
    app: &AppHandle,
    req: ShortcutCreateRequest,
) -> Result<ShortcutCreateResult, String> {
    let (external_game_dir, external_root_dir) = normalize_external_dirs(&req.selected_path);
    if !external_game_dir.exists() {
        return Err(format!(
            "External gameDir no existe: {}",
            external_game_dir.display()
        ));
    }
    let instances_root = crate::app::settings_service::resolve_instances_root(app)?;
    fs::create_dir_all(&instances_root)
        .map_err(|e| format!("No se pudo crear instances root: {e}"))?;
    let mut sanitized = crate::infrastructure::filesystem::paths::sanitize_path_segment(&req.name);
    if sanitized.trim().is_empty() {
        sanitized = "instancia-atajo".to_string();
    }
    let mut instance_root = instances_root.join(&sanitized);
    let mut i = 1;
    while instance_root.exists() {
        instance_root = instances_root.join(format!("{sanitized}-{i}"));
        i += 1;
    }
    let runtime_root = instance_root.join("runtime");
    for dir in [
        runtime_root.join("libraries"),
        runtime_root.join("assets"),
        runtime_root.join("versions"),
        runtime_root.join("loader"),
        instance_root.join("natives"),
        instance_root.join("logs"),
    ] {
        fs::create_dir_all(&dir).map_err(|e| format!("No se pudo crear {}: {e}", dir.display()))?;
    }

    let (mut mc_version, mut loader, mut loader_version) =
        read_instance_manifest_strict(&external_root_dir);
    if mc_version.is_empty() {
        mc_version = req.fallback_mc;
    }
    if loader.is_empty() {
        loader = req.fallback_loader;
    }
    if loader_version.is_empty() || loader_version == "-" {
        loader_version = req.fallback_loader_version;
    }
    if mc_version.trim().is_empty() {
        return Err("No se pudo detectar mc_version para el atajo.".to_string());
    }

    let now = chrono::Utc::now().to_rfc3339();
    let mut state = ShortcutState {
        id: uuid::Uuid::new_v4().to_string(),
        name: req.name.clone(),
        external_game_dir: external_game_dir.display().to_string(),
        external_root_dir: external_root_dir.display().to_string(),
        mc_version: mc_version.clone(),
        loader: loader.clone(),
        loader_version: loader_version.clone(),
        status: "CREATING".to_string(),
        created_at: now.clone(),
        updated_at: now,
        adopt_mode: "off".to_string(),
        locator: ExternalLocator {
            last_known_path: external_game_dir.display().to_string(),
            signature: compute_signature(&external_game_dir, &external_root_dir),
            hints: vec![req.source_launcher.clone(), loader.clone()],
            scan_roots: vec![external_root_dir
                .parent()
                .unwrap_or(&external_root_dir)
                .display()
                .to_string()],
        },
        launch_plan: ShortcutLaunchPlan::default(),
    };
    save_shortcut_state(&instance_root, &state)?;

    let runtime = runtime_for_mc(&mc_version, &loader)?;
    let mut logs = vec!["[SHORTCUT][create] ensure runtime interno".to_string()];
    let java_exec = select_embedded_java(app, runtime, &mut logs)?;
    let effective_version_id = build_instance_structure(
        &instance_root,
        &runtime_root,
        &mc_version,
        &loader,
        &loader_version,
        &java_exec,
        &mut logs,
        &mut |_| {},
    )
    .map_err(|e| format!("Fallo ensure runtime base: {e}"))?;
    if loader.eq_ignore_ascii_case("forge") {
        let _ = install_loader_if_needed(
            &runtime_root,
            &mc_version,
            &loader,
            &loader_version,
            &java_exec,
            &mut logs,
        )
        .map_err(|e| format!("Forge processors no ejecutados; jars faltantes: {e}"))?;
    }

    let plan = build_launch_plan(
        app,
        &state,
        &runtime_root,
        &effective_version_id,
        &java_exec,
    )?;
    validate_preflight(&plan)?;

    let metadata = crate::domain::models::instance::InstanceMetadata {
        name: req.name,
        group: req.target_group,
        minecraft_version: mc_version,
        version_id: effective_version_id,
        loader,
        loader_version,
        ram_mb: 4096,
        java_args: vec!["-XX:+UnlockExperimentalVMOptions".to_string()],
        java_path: java_exec.display().to_string(),
        java_runtime: "shortcut".to_string(),
        java_version: String::new(),
        required_java_major: u32::from(runtime.major()),
        created_at: state.created_at.clone(),
        state: "REDIRECT".to_string(),
        last_used: None,
        internal_uuid: state.id.clone(),
    };
    fs::write(
        instance_root.join(".instance.json"),
        serde_json::to_vec_pretty(&metadata).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::write(instance_root.join(".redirect.json"), serde_json::to_vec_pretty(&serde_json::json!({"sourcePath": external_root_dir.display().to_string(), "sourceLauncher": req.source_launcher})).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;

    state.status = "READY".to_string();
    state.updated_at = chrono::Utc::now().to_rfc3339();
    state.launch_plan = plan;
    save_shortcut_state(&instance_root, &state)?;
    Ok(ShortcutCreateResult { instance_root })
}

pub fn build_launch_plan(
    app: &AppHandle,
    state: &ShortcutState,
    runtime_root: &Path,
    version_id: &str,
    java_exec: &Path,
) -> Result<ShortcutLaunchPlan, String> {
    let versions_root = runtime_root.join("versions");
    let version_json = merge_version_json_chain(&versions_root, version_id)?;
    let classpath = build_classpath_multi(
        &version_json,
        &[runtime_root.join("libraries")],
        &versions_root,
        version_id,
    )?;
    let cp_entries: Vec<String> = classpath
        .split(if cfg!(windows) { ';' } else { ':' })
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
        .collect();

    let assets_index = version_json
        .get("assetIndex")
        .and_then(Value::as_object)
        .and_then(|o| o.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("legacy")
        .to_string();
    let assets_root = runtime_root.join("assets");
    let natives_dir = runtime_root
        .parent()
        .unwrap_or(runtime_root)
        .join("natives");
    let _ = fs::remove_dir_all(&natives_dir);
    tauri::async_runtime::block_on(prepare_redirect_natives(
        app,
        &version_json,
        version_id,
        &runtime_root.join("libraries"),
        runtime_root,
        &natives_dir,
        "shortcut",
    ))?;

    let launch = LaunchContext {
        classpath,
        classpath_separator: if cfg!(windows) {
            ";".to_string()
        } else {
            ":".to_string()
        },
        library_directory: runtime_root.join("libraries").display().to_string(),
        natives_dir: natives_dir.display().to_string(),
        launcher_name: "Interface".to_string(),
        launcher_version: "1.0".to_string(),
        auth_player_name: "Player".to_string(),
        auth_uuid: "00000000-0000-0000-0000-000000000000".to_string(),
        auth_access_token: "0".to_string(),
        user_type: "msa".to_string(),
        user_properties: "{}".to_string(),
        version_name: version_id.to_string(),
        game_directory: state.external_game_dir.clone(),
        assets_root: assets_root.display().to_string(),
        assets_index_name: assets_index.clone(),
        version_type: "release".to_string(),
        resolution_width: "1280".to_string(),
        resolution_height: "720".to_string(),
        clientid: "".to_string(),
        auth_xuid: "".to_string(),
        xuid: "".to_string(),
        quick_play_singleplayer: "".to_string(),
        quick_play_multiplayer: "".to_string(),
        quick_play_realms: "".to_string(),
        quick_play_path: "".to_string(),
    };
    let resolved = resolve_launch_arguments(&version_json, &launch, &RuleContext::current())?;
    Ok(ShortcutLaunchPlan {
        java_path: java_exec.display().to_string(),
        main_class: resolved.main_class,
        jvm_args: resolved.jvm,
        game_args: resolved.game,
        classpath: cp_entries,
        assets_root: assets_root.display().to_string(),
        asset_index: assets_index,
        natives_dir: natives_dir.display().to_string(),
        libraries_root: runtime_root.join("libraries").display().to_string(),
        versions_root: versions_root.display().to_string(),
        version_id: version_id.to_string(),
    })
}

fn runtime_for_mc(mc: &str, loader: &str) -> Result<JavaRuntime, String> {
    determine_required_java(mc, loader)
}

fn merge_version_json_chain(versions_root: &Path, version_id: &str) -> Result<Value, String> {
    let path = versions_root
        .join(version_id)
        .join(format!("{version_id}.json"));
    let raw = fs::read_to_string(&path)
        .map_err(|e| format!("No se pudo leer version json {}: {e}", path.display()))?;
    let current: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    if let Some(parent) = current.get("inheritsFrom").and_then(Value::as_str) {
        let p = merge_version_json_chain(versions_root, parent)?;
        Ok(merge_json(&p, &current))
    } else {
        Ok(current)
    }
}

fn merge_json(parent: &Value, child: &Value) -> Value {
    let mut merged = parent.clone();
    if let (Some(pm), Some(cm)) = (merged.as_object_mut(), child.as_object()) {
        for (k, v) in cm {
            if k == "libraries" {
                let mut arr = pm
                    .get(k)
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                arr.extend(v.as_array().cloned().unwrap_or_default());
                pm.insert(k.clone(), Value::Array(arr));
            } else {
                pm.insert(k.clone(), v.clone());
            }
        }
    }
    merged
}

pub fn validate_preflight(plan: &ShortcutLaunchPlan) -> Result<(), String> {
    if plan.main_class.trim().is_empty()
        || plan.classpath.is_empty()
        || plan.asset_index.trim().is_empty()
        || plan.assets_root.trim().is_empty()
        || plan.java_path.trim().is_empty()
        || plan.natives_dir.trim().is_empty()
    {
        return Err("launch_plan incompleto en campos críticos".to_string());
    }
    if !Path::new(&plan.java_path).exists() {
        return Err(format!("java_path inválido {}", plan.java_path));
    }
    if !Path::new(&plan.assets_root)
        .join("indexes")
        .join(format!("{}.json", plan.asset_index))
        .exists()
    {
        return Err("assetIndex no existe".to_string());
    }
    if fs::read_dir(&plan.natives_dir)
        .map_err(|_| "natives inexistente".to_string())?
        .next()
        .is_none()
    {
        return Err("natives_dir vacío".to_string());
    }
    let missing: Vec<String> = plan
        .classpath
        .iter()
        .filter(|p| !Path::new(p).exists())
        .take(20)
        .cloned()
        .collect();
    if !missing.is_empty() {
        return Err(format!("Classpath faltante: {}", missing.join(" | ")));
    }
    Ok(())
}

pub fn refresh_auth_args(
    plan: &ShortcutLaunchPlan,
    auth: &LaunchAuthSession,
    game_dir: &str,
) -> Vec<String> {
    let mut args = Vec::with_capacity(plan.game_args.len());
    let uuid_no_dash = auth.profile_id.replace('-', "");
    for a in &plan.game_args {
        args.push(
            a.replace("Player", &auth.profile_name)
                .replace("00000000-0000-0000-0000-000000000000", &uuid_no_dash)
                .replace("--accessToken 0", "--accessToken")
                .replace(" 0", &format!(" {}", auth.minecraft_access_token))
                .replace(game_dir, game_dir),
        );
    }
    args
}

pub fn normalize_external_root(external_game_dir: &Path) -> PathBuf {
    let (game, root) = normalize_external_dirs(external_game_dir);
    let _ = game;
    root
}

pub fn compute_signature(game_dir: &Path, root_dir: &Path) -> Signature {
    let mut hasher = Sha1::new();
    let files = [
        root_dir.join("minecraftinstance.json"),
        game_dir.join("pack.mcmeta"),
        game_dir.join("options.txt"),
    ];
    for path in &files {
        if let Ok(meta) = fs::metadata(path) {
            hasher.update(path.display().to_string().as_bytes());
            hasher.update(meta.len().to_le_bytes());
            if meta.len() <= 1024 * 128 {
                if let Ok(bytes) = fs::read(path) {
                    hasher.update(bytes);
                }
            }
        }
    }
    Signature {
        has_minecraftinstance_json: root_dir.join("minecraftinstance.json").exists(),
        has_pack_mcmeta: game_dir.join("pack.mcmeta").exists(),
        has_options_txt: game_dir.join("options.txt").exists(),
        fingerprint: format!("{:x}", hasher.finalize()),
    }
}

pub fn save_shortcut_state(instance_root: &Path, state: &ShortcutState) -> Result<(), String> {
    let raw = serde_json::to_string_pretty(state)
        .map_err(|err| format!("No se pudo serializar state.json del atajo: {err}"))?;
    fs::write(instance_root.join("state.json"), raw)
        .map_err(|err| format!("No se pudo guardar state.json del atajo: {err}"))
}

pub fn validate_classpath_exists(classpath: &[PathBuf]) -> Result<(), Vec<PathBuf>> {
    let missing: Vec<PathBuf> = classpath.iter().filter(|p| !p.exists()).cloned().collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(missing)
    }
}

pub fn select_embedded_java(
    app: &tauri::AppHandle,
    runtime: JavaRuntime,
    logs: &mut Vec<String>,
) -> Result<PathBuf, String> {
    let launcher_root = resolve_launcher_root(app).map_err(|err| err.to_string())?;
    let java_exec = ensure_embedded_java(&launcher_root, runtime, logs)?;
    let output = Command::new(&java_exec)
        .arg("-version")
        .output()
        .map_err(|err| {
            format!(
                "No se pudo ejecutar java -version ({}) : {err}",
                java_exec.display()
            )
        })?;
    if !output.status.success() {
        return Err(format!(
            "Java embebido inválido en {} (exit={})",
            java_exec.display(),
            output.status
        ));
    }
    Ok(java_exec)
}

pub fn resolve_external_game_dir_with_relink(
    locator: &ExternalLocator,
    max_dirs: usize,
    timeout: Duration,
) -> Option<PathBuf> {
    let initial = PathBuf::from(&locator.last_known_path);
    if initial.exists() {
        return Some(initial);
    }
    let mut roots: Vec<PathBuf> = locator.scan_roots.iter().map(PathBuf::from).collect();
    if let Some(home) = std::env::var_os("USERPROFILE") {
        roots.push(
            PathBuf::from(home)
                .join("AppData")
                .join("Roaming")
                .join("PrismLauncher")
                .join("instances"),
        );
    }
    let started = Instant::now();
    let mut visited = 0usize;
    for root in roots {
        if !root.exists() {
            continue;
        }
        let mut queue = VecDeque::new();
        queue.push_back(root);
        while let Some(dir) = queue.pop_front() {
            if started.elapsed() > timeout || visited >= max_dirs {
                return None;
            }
            visited += 1;
            let candidate_game = dir.join("minecraft");
            if candidate_game.exists() {
                let normalized = normalize_external_root(&candidate_game);
                let candidate_sig = compute_signature(&candidate_game, &normalized);
                if candidate_sig.fingerprint == locator.signature.fingerprint {
                    return Some(candidate_game);
                }
            }
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        queue.push_back(path);
                    }
                }
            }
        }
    }
    None
}

pub fn build_java_args(
    classpath_entries: &[PathBuf],
    natives_dir: &Path,
    main_class: &str,
    mut jvm_args: Vec<String>,
    game_args: Vec<String>,
) -> Vec<String> {
    let sep = if cfg!(target_os = "windows") {
        ";"
    } else {
        ":"
    };
    let classpath = classpath_entries
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<String>>()
        .join(sep);
    jvm_args.push(format!("-Djava.library.path={}", natives_dir.display()));
    jvm_args.push("-cp".to_string());
    jvm_args.push(classpath);
    jvm_args.push(main_class.to_string());
    jvm_args.extend(game_args);
    jvm_args
}

fn read_instance_manifest_strict(source_root: &Path) -> (String, String, String) {
    let manifest_path = source_root.join("minecraftinstance.json");
    if !manifest_path.exists() {
        return (String::new(), String::new(), String::new());
    }
    let raw = match fs::read_to_string(&manifest_path) {
        Ok(v) => v,
        Err(_) => return (String::new(), String::new(), String::new()),
    };
    let json: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return (String::new(), String::new(), String::new()),
    };
    let mc = json
        .get("mcVersion")
        .or_else(|| json.pointer("/components/0/version"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let loader = json
        .get("loader")
        .or_else(|| json.pointer("/components/1/uid"))
        .and_then(Value::as_str)
        .unwrap_or("vanilla")
        .to_string();
    let lv = json
        .get("loaderVersion")
        .or_else(|| json.pointer("/components/1/version"))
        .and_then(Value::as_str)
        .unwrap_or("-")
        .to_string();
    (mc, loader, lv)
}
