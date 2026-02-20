use std::{fs, path::PathBuf};

use tauri::{path::BaseDirectory, AppHandle, Manager};

use crate::shared::result::AppResult;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(default)]
pub struct LauncherConfig {
    pub launcher_root_override: Option<String>,
    pub instances_dir_override: Option<String>,
}

pub fn launcher_config_path(app: &AppHandle) -> AppResult<PathBuf> {
    app.path()
        .resolve(
            "InterfaceLauncher/launcher_config.json",
            BaseDirectory::AppConfig,
        )
        .map_err(|err| err.to_string())
}

pub fn load_launcher_config(app: &AppHandle) -> AppResult<LauncherConfig> {
    let path = launcher_config_path(app)?;
    if !path.exists() {
        return Ok(LauncherConfig::default());
    }

    let raw = fs::read_to_string(&path).map_err(|err| {
        format!(
            "No se pudo leer launcher_config.json {}: {err}",
            path.display()
        )
    })?;

    serde_json::from_str::<LauncherConfig>(&raw).map_err(|err| {
        format!(
            "No se pudo parsear launcher_config.json {}: {err}",
            path.display()
        )
    })
}

pub fn save_launcher_config(app: &AppHandle, config: &LauncherConfig) -> AppResult<()> {
    let path = launcher_config_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "No se pudo crear directorio para launcher_config.json {}: {err}",
                parent.display()
            )
        })?;
    }

    let raw = serde_json::to_string_pretty(config)
        .map_err(|err| format!("No se pudo serializar launcher_config.json: {err}"))?;

    fs::write(&path, raw).map_err(|err| {
        format!(
            "No se pudo guardar launcher_config.json {}: {err}",
            path.display()
        )
    })?;

    Ok(())
}
