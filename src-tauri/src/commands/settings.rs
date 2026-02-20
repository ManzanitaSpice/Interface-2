use std::{
    fs,
    path::{Path, PathBuf},
};

use fs2::available_space;
use tauri::{AppHandle, Emitter};

use crate::{
    app::{
        instance_service::has_running_instances, launcher_service::list_instances,
        settings_service::resolve_instances_root,
    },
    infrastructure::filesystem::{
        config::{load_launcher_config, save_launcher_config, LauncherConfig},
        paths::resolve_launcher_root,
    },
};

#[derive(serde::Serialize)]
pub struct LauncherFolders {
    pub launcher_root: String,
    pub instances_dir: String,
    pub runtime_dir: String,
    pub assets_dir: String,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct MigrationProgressEvent {
    step: String,
    completed: usize,
    total: usize,
    message: String,
}

fn ensure_valid_destination(source: &Path, target: &Path) -> Result<(), String> {
    if source == target {
        return Err("La carpeta destino no puede ser la misma que la actual.".to_string());
    }
    if target.starts_with(source) {
        return Err("La carpeta destino no puede estar dentro de la carpeta origen.".to_string());
    }
    Ok(())
}

fn dir_size(path: &Path) -> Result<u64, String> {
    if !path.exists() {
        return Ok(0);
    }
    let mut size = 0u64;
    for entry in
        fs::read_dir(path).map_err(|e| format!("No se pudo leer {}: {e}", path.display()))?
    {
        let entry = entry.map_err(|e| format!("No se pudo leer entrada: {e}"))?;
        let p = entry.path();
        if p.is_dir() {
            size = size.saturating_add(dir_size(&p)?);
        } else {
            size = size.saturating_add(
                p.metadata()
                    .map_err(|e| format!("No se pudo leer metadata: {e}"))?
                    .len(),
            );
        }
    }
    Ok(size)
}

fn copy_recursive_with_progress(
    app: &AppHandle,
    from: &Path,
    to: &Path,
    completed: &mut usize,
    total: usize,
    step: &str,
) -> Result<(), String> {
    if from.is_dir() {
        fs::create_dir_all(to).map_err(|e| format!("No se pudo crear {}: {e}", to.display()))?;
        for entry in
            fs::read_dir(from).map_err(|e| format!("No se pudo leer {}: {e}", from.display()))?
        {
            let entry = entry.map_err(|e| format!("No se pudo leer entrada: {e}"))?;
            copy_recursive_with_progress(
                app,
                &entry.path(),
                &to.join(entry.file_name()),
                completed,
                total,
                step,
            )?;
        }
    } else {
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("No se pudo crear {}: {e}", parent.display()))?;
        }
        fs::copy(from, to).map_err(|e| {
            format!(
                "No se pudo copiar {} -> {}: {e}",
                from.display(),
                to.display()
            )
        })?;
        *completed += 1;
        let _ = app.emit(
            "migration_progress",
            MigrationProgressEvent {
                step: step.to_string(),
                completed: *completed,
                total,
                message: format!("Copiando {}", from.display()),
            },
        );
    }
    Ok(())
}

fn list_files_count(path: &Path) -> Result<usize, String> {
    if !path.exists() {
        return Ok(0);
    }
    let mut total = 0usize;
    for entry in
        fs::read_dir(path).map_err(|e| format!("No se pudo leer {}: {e}", path.display()))?
    {
        let entry = entry.map_err(|e| format!("No se pudo leer entrada: {e}"))?;
        let p = entry.path();
        if p.is_dir() {
            total += list_files_count(&p)?;
        } else {
            total += 1;
        }
    }
    Ok(total)
}

#[tauri::command]
pub fn get_launcher_folders(app: AppHandle) -> Result<LauncherFolders, String> {
    let launcher_root = resolve_launcher_root(&app)?;
    let instances_dir = resolve_instances_root(&app)?;
    Ok(LauncherFolders {
        launcher_root: launcher_root.display().to_string(),
        instances_dir: instances_dir.display().to_string(),
        runtime_dir: launcher_root.join("runtime").display().to_string(),
        assets_dir: launcher_root.join("assets").display().to_string(),
    })
}

#[tauri::command]
pub fn get_instances_count(app: AppHandle) -> Result<u32, String> {
    Ok(list_instances(app)?.len() as u32)
}

#[tauri::command]
pub fn migrate_launcher_root(
    app: AppHandle,
    new_path: String,
    migrate_files: bool,
) -> Result<(), String> {
    if has_running_instances()? {
        return Err("Hay instancias en ejecución. Cierra los juegos antes de migrar.".to_string());
    }

    let old_root = resolve_launcher_root(&app)?;
    let new_root = PathBuf::from(new_path.trim());
    ensure_valid_destination(&old_root, &new_root)?;

    if migrate_files {
        let required = dir_size(&old_root)?.saturating_add(500 * 1024 * 1024);
        let free = available_space(&new_root)
            .or_else(|_| available_space(new_root.parent().unwrap_or(&new_root)))
            .map_err(|e| format!("No se pudo verificar espacio disponible: {e}"))?;
        if free < required {
            return Err("No hay suficiente espacio libre para migrar el launcher.".to_string());
        }

        let total = list_files_count(&old_root)?;
        let mut completed = 0usize;
        copy_recursive_with_progress(
            &app,
            &old_root,
            &new_root,
            &mut completed,
            total.max(1),
            "migrating_launcher_root",
        )?;
    }

    let mut config = load_launcher_config(&app).unwrap_or_else(|_| LauncherConfig::default());
    config.launcher_root_override = Some(new_root.display().to_string());
    save_launcher_config(&app, &config)?;

    Ok(())
}

#[tauri::command]
pub fn change_instances_folder(
    app: AppHandle,
    new_path: String,
    migrate_files: bool,
) -> Result<(), String> {
    if has_running_instances()? {
        return Err("Hay instancias en ejecución. Cierra los juegos antes de migrar.".to_string());
    }

    let current = resolve_instances_root(&app)?;
    let target = PathBuf::from(new_path.trim());
    ensure_valid_destination(&current, &target)?;

    fs::create_dir_all(&target)
        .map_err(|e| format!("No se pudo crear destino {}: {e}", target.display()))?;

    if migrate_files && current.exists() {
        let entries: Vec<_> = fs::read_dir(&current)
            .map_err(|e| format!("No se pudo leer instancias: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("No se pudo leer entrada de instancia: {e}"))?;
        let total = entries.len().max(1);

        for (idx, entry) in entries.iter().enumerate() {
            let from = entry.path();
            let to = target.join(entry.file_name());
            if to.exists() {
                continue;
            }
            fs::rename(&from, &to).or_else(|_| {
                let mut completed = 0usize;
                copy_recursive_with_progress(
                    &app,
                    &from,
                    &to,
                    &mut completed,
                    list_files_count(&from)?.max(1),
                    "moving_instance",
                )?;
                if from.is_dir() {
                    fs::remove_dir_all(&from)
                        .map_err(|e| format!("No se pudo eliminar {}: {e}", from.display()))?;
                }
                Ok::<(), String>(())
            })?;

            let _ = app.emit(
                "migration_progress",
                MigrationProgressEvent {
                    step: "moving_instances".to_string(),
                    completed: idx + 1,
                    total,
                    message: format!("Moviendo {}", to.display()),
                },
            );
        }
    }

    let mut config = load_launcher_config(&app).unwrap_or_else(|_| LauncherConfig::default());
    config.instances_dir_override = Some(target.display().to_string());
    save_launcher_config(&app, &config)?;

    Ok(())
}
