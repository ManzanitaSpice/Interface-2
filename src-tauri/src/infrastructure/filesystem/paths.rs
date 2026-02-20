use std::{
    fs,
    path::{Path, PathBuf},
};

use tauri::{path::BaseDirectory, Manager};

use crate::{infrastructure::filesystem::config::load_launcher_config, shared::result::AppResult};

pub fn resolve_launcher_root(app: &tauri::AppHandle) -> AppResult<PathBuf> {
    let default = default_launcher_root(app)?;

    if let Ok(config) = load_launcher_config(app) {
        if let Some(override_path) = config.launcher_root_override {
            let candidate = PathBuf::from(override_path.trim());
            if !candidate.as_os_str().is_empty() && candidate.exists() {
                return Ok(candidate);
            }
        }
    }

    let file = folder_routes_settings_file(app)?;
    if !file.exists() {
        return Ok(default);
    }

    let raw = fs::read_to_string(&file).map_err(|err| {
        format!(
            "No se pudo leer configuración global de rutas {}: {err}",
            file.display()
        )
    })?;

    let parsed = serde_json::from_str::<serde_json::Value>(&raw).map_err(|err| {
        format!(
            "No se pudo parsear configuración global de rutas {}: {err}",
            file.display()
        )
    })?;

    let configured = parsed
        .get("routes")
        .and_then(|routes| routes.as_array())
        .and_then(|routes| {
            routes.iter().find_map(|route| {
                let key = route.get("key")?.as_str()?;
                if key != "launcher" {
                    return None;
                }
                route
                    .get("value")
                    .and_then(|value| value.as_str())
                    .map(|value| PathBuf::from(value.trim()))
            })
        })
        .filter(|value| !value.as_os_str().is_empty());

    Ok(configured.unwrap_or(default))
}

pub fn default_launcher_root(app: &tauri::AppHandle) -> AppResult<PathBuf> {
    app.path()
        .resolve("InterfaceLauncher", BaseDirectory::AppData)
        .map_err(|err| err.to_string())
}

pub fn folder_routes_settings_file(app: &tauri::AppHandle) -> AppResult<PathBuf> {
    let settings_root = app
        .path()
        .resolve("InterfaceLauncher", BaseDirectory::AppConfig)
        .map_err(|err| err.to_string())?;
    Ok(settings_root.join("config").join("folder_routes.json"))
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
