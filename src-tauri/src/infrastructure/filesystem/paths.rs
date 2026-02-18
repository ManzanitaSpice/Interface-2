use std::path::{Path, PathBuf};

use tauri::{path::BaseDirectory, Manager};

use crate::shared::result::AppResult;

pub fn resolve_launcher_root(app: &tauri::AppHandle) -> AppResult<PathBuf> {
    app.path()
        .resolve("InterfaceLauncher", BaseDirectory::AppData)
        .map_err(|err| err.to_string())
}

pub fn sanitize_path_segment(value: &str) -> String {
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

pub fn java_executable_path(runtime_root: &Path) -> PathBuf {
    if cfg!(target_os = "windows") {
        runtime_root.join("bin").join("java.exe")
    } else {
        runtime_root.join("bin").join("java")
    }
}
