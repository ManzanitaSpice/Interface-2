use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use tauri::AppHandle;

use crate::infrastructure::filesystem::paths::{
    default_launcher_root, folder_routes_settings_file, resolve_launcher_root,
};

#[derive(serde::Serialize)]
pub struct PickedFolderResult {
    pub path: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderRouteMigrationResult {
    pub moved_entries: usize,
    pub skipped_entries: usize,
    pub copied_entries: usize,
    pub target_path: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderRoutesPayload {
    pub routes: Vec<FolderRouteInput>,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderRouteInput {
    pub key: String,
    pub value: String,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct FolderRouteFile {
    routes: Vec<FolderRouteInput>,
}

fn folder_routes_file(app: &AppHandle) -> Result<PathBuf, String> {
    folder_routes_settings_file(app)
}

fn normalize_path(path: &str) -> String {
    path.trim().replace('\\', "/")
}

pub fn resolve_folder_route<F>(
    app: &AppHandle,
    key: &str,
    default_builder: F,
) -> Result<PathBuf, String>
where
    F: FnOnce(&Path) -> PathBuf,
{
    let launcher_root = resolve_launcher_root(app)?;
    let default = default_builder(&launcher_root);
    let path = folder_routes_file(app)?;
    if !path.exists() {
        return Ok(default);
    }

    let raw = fs::read_to_string(&path).map_err(|err| {
        format!(
            "No se pudo leer configuración de carpetas {}: {err}",
            path.display()
        )
    })?;
    let parsed: FolderRouteFile = serde_json::from_str(&raw).map_err(|err| {
        format!(
            "No se pudo parsear configuración de carpetas {}: {err}",
            path.display()
        )
    })?;
    let route = parsed
        .routes
        .into_iter()
        .find(|route| route.key == key)
        .map(|route| route.value)
        .unwrap_or_else(|| default.display().to_string());

    Ok(PathBuf::from(normalize_path(&route)))
}

pub fn resolve_instances_root(app: &AppHandle) -> Result<PathBuf, String> {
    resolve_folder_route(app, "instances", |launcher_root| {
        launcher_root.join("instances")
    })
}

#[tauri::command]
pub fn pick_folder(
    initial_path: Option<String>,
    title: Option<String>,
) -> Result<PickedFolderResult, String> {
    let mut dialog = rfd::FileDialog::new();

    if let Some(path) = initial_path {
        let sanitized = path.trim();
        if !sanitized.is_empty() {
            dialog = dialog.set_directory(PathBuf::from(sanitized));
        }
    }

    if let Some(custom_title) = title {
        let sanitized = custom_title.trim();
        if !sanitized.is_empty() {
            dialog = dialog.set_title(sanitized);
        }
    }

    let selected = dialog.pick_folder();
    Ok(PickedFolderResult {
        path: selected.map(|folder| folder.to_string_lossy().to_string()),
    })
}

fn default_routes(app: &AppHandle) -> Result<Vec<FolderRouteInput>, String> {
    let launcher_root = default_launcher_root(app)?;
    Ok(vec![
        FolderRouteInput {
            key: "launcher".to_string(),
            value: launcher_root.display().to_string(),
        },
        FolderRouteInput {
            key: "instances".to_string(),
            value: launcher_root.join("instances").display().to_string(),
        },
        FolderRouteInput {
            key: "icons".to_string(),
            value: launcher_root
                .join("assets")
                .join("icons")
                .display()
                .to_string(),
        },
        FolderRouteInput {
            key: "java".to_string(),
            value: launcher_root.join("runtime").display().to_string(),
        },
        FolderRouteInput {
            key: "skins".to_string(),
            value: launcher_root
                .join("assets")
                .join("skins")
                .display()
                .to_string(),
        },
        FolderRouteInput {
            key: "downloads".to_string(),
            value: launcher_root.join("downloads").display().to_string(),
        },
    ])
}

#[tauri::command]
pub fn load_folder_routes(app: AppHandle) -> Result<FolderRoutesPayload, String> {
    let mut routes = default_routes(&app)?;
    let config_path = folder_routes_file(&app)?;
    if !config_path.exists() {
        return Ok(FolderRoutesPayload { routes });
    }

    let raw = fs::read_to_string(&config_path).map_err(|err| {
        format!(
            "No se pudo leer configuración de carpetas {}: {err}",
            config_path.display()
        )
    })?;
    let parsed: FolderRouteFile = serde_json::from_str(&raw).map_err(|err| {
        format!(
            "No se pudo parsear configuración de carpetas {}: {err}",
            config_path.display()
        )
    })?;

    for route in &mut routes {
        if let Some(saved) = parsed.routes.iter().find(|saved| saved.key == route.key) {
            route.value = normalize_path(&saved.value);
        }
    }

    Ok(FolderRoutesPayload { routes })
}

fn normalize_routes_payload(
    app: &AppHandle,
    routes: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let launcher_root = default_launcher_root(app)?;
    let mut parsed: FolderRouteFile = serde_json::from_value(routes)
        .map_err(|err| format!("Formato inválido en rutas de carpetas: {err}"))?;

    for route in &mut parsed.routes {
        let cleaned = normalize_path(&route.value);
        route.value = if route.key == "launcher" || Path::new(&cleaned).is_absolute() {
            cleaned
        } else {
            launcher_root.join(cleaned).display().to_string()
        };
    }

    serde_json::to_value(parsed)
        .map_err(|err| format!("No se pudo normalizar rutas de carpetas: {err}"))
}

#[tauri::command]
pub fn save_folder_routes(app: AppHandle, routes: serde_json::Value) -> Result<(), String> {
    let target = folder_routes_file(&app)?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "No se pudo preparar carpeta de configuración {}: {err}",
                parent.display()
            )
        })?;
    }
    let normalized = normalize_routes_payload(&app, routes)?;
    let pretty = serde_json::to_string_pretty(&normalized)
        .map_err(|err| format!("No se pudo serializar configuración de carpetas: {err}"))?;
    fs::write(&target, pretty).map_err(|err| {
        format!(
            "No se pudo guardar configuración de carpetas {}: {err}",
            target.display()
        )
    })
}

#[tauri::command]
pub fn open_folder_path(path: String) -> Result<(), String> {
    let target = PathBuf::from(normalize_path(&path));
    if target.as_os_str().is_empty() {
        return Err("Ruta de carpeta vacía".to_string());
    }

    if !target.exists() {
        fs::create_dir_all(&target).map_err(|err| {
            format!(
                "La carpeta no existe y no se pudo crear {}: {err}",
                target.display()
            )
        })?;
    }

    if !target.is_dir() {
        return Err(format!("La ruta no es una carpeta: {}", target.display()));
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(&target)
            .status()
            .map_err(|err| format!("No se pudo abrir el explorador de Windows: {err}"))?;
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(&target)
            .status()
            .map_err(|err| format!("No se pudo abrir Finder: {err}"))?;
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(&target)
            .status()
            .map_err(|err| format!("No se pudo abrir el explorador de archivos: {err}"))?;
    }

    Ok(())
}

#[tauri::command]
pub fn migrate_instances_folder(
    _app: AppHandle,
    source_path: String,
    target_path: String,
) -> Result<FolderRouteMigrationResult, String> {
    let source = PathBuf::from(normalize_path(&source_path));
    let target = PathBuf::from(normalize_path(&target_path));

    fs::create_dir_all(&target).map_err(|err| {
        format!(
            "No se pudo preparar carpeta de destino {}: {err}",
            target.display()
        )
    })?;

    if source == target {
        return Ok(FolderRouteMigrationResult {
            moved_entries: 0,
            skipped_entries: 0,
            copied_entries: 0,
            target_path: target.display().to_string(),
        });
    }

    if !source.exists() {
        return Ok(FolderRouteMigrationResult {
            moved_entries: 0,
            skipped_entries: 0,
            copied_entries: 0,
            target_path: target.display().to_string(),
        });
    }

    let mut moved_entries = 0usize;
    let mut skipped_entries = 0usize;
    let mut copied_entries = 0usize;

    for entry in fs::read_dir(&source).map_err(|err| {
        format!(
            "No se pudo leer carpeta de instancias {}: {err}",
            source.display()
        )
    })? {
        let entry = entry.map_err(|err| format!("No se pudo leer elemento de instancia: {err}"))?;
        let from = entry.path();
        let to = target.join(entry.file_name());
        if to.exists() {
            skipped_entries += 1;
            continue;
        }
        match fs::rename(&from, &to) {
            Ok(_) => moved_entries += 1,
            Err(_) => {
                copy_path_recursive(&from, &to)?;
                if from.is_dir() {
                    fs::remove_dir_all(&from).map_err(|err| {
                        format!(
                            "No se pudo limpiar carpeta original {}: {err}",
                            from.display()
                        )
                    })?;
                } else {
                    fs::remove_file(&from).map_err(|err| {
                        format!(
                            "No se pudo limpiar archivo original {}: {err}",
                            from.display()
                        )
                    })?;
                }
                copied_entries += 1;
            }
        }
    }

    Ok(FolderRouteMigrationResult {
        moved_entries,
        skipped_entries,
        copied_entries,
        target_path: target.display().to_string(),
    })
}

fn copy_path_recursive(from: &Path, to: &Path) -> Result<(), String> {
    if from.is_dir() {
        fs::create_dir_all(to).map_err(|err| {
            format!(
                "No se pudo crear carpeta de destino {}: {err}",
                to.display()
            )
        })?;
        for entry in fs::read_dir(from)
            .map_err(|err| format!("No se pudo leer carpeta fuente {}: {err}", from.display()))?
        {
            let entry =
                entry.map_err(|err| format!("No se pudo leer entrada de carpeta: {err}"))?;
            let child_from = entry.path();
            let child_to = to.join(entry.file_name());
            copy_path_recursive(&child_from, &child_to)?;
        }
        Ok(())
    } else {
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "No se pudo crear carpeta padre de destino {}: {err}",
                    parent.display()
                )
            })?;
        }
        fs::copy(from, to).map_err(|err| {
            format!(
                "No se pudo copiar {} -> {}: {err}",
                from.display(),
                to.display()
            )
        })?;
        Ok(())
    }
}
