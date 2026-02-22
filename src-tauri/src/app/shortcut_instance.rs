use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use crate::{
    domain::models::java::JavaRuntime, infrastructure::filesystem::paths::resolve_launcher_root,
    services::java_installer::ensure_embedded_java,
};

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
    pub created_at: String,
    pub updated_at: String,
    pub adopt_mode: String,
    pub locator: ExternalLocator,
}

pub fn normalize_external_root(external_game_dir: &Path) -> PathBuf {
    let game = external_game_dir;
    let file_name = game
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    if file_name == "minecraft" {
        if let Some(parent) = game.parent() {
            if parent.join("minecraftinstance.json").exists() {
                return parent.to_path_buf();
            }
        }
    }

    if game.join("minecraftinstance.json").exists() {
        return game.to_path_buf();
    }

    if let Some(parent) = game.parent() {
        if parent.join("minecraftinstance.json").exists() {
            return parent.to_path_buf();
        }
    }

    game.to_path_buf()
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
