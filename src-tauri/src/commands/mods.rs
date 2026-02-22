use serde::Serialize;
use std::{fs, path::PathBuf, time::UNIX_EPOCH};

fn section_folder(section: Option<&str>) -> &'static str {
    match section
        .unwrap_or("mods")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "shaderpacks" | "shaders" | "shader" => "shaderpacks",
        "resourcepacks" | "resourcepack" | "resource" => "resourcepacks",
        "worlds" | "mundos" | "world" => "saves",
        _ => "mods",
    }
}

fn section_allows_disable(section: Option<&str>) -> bool {
    section_folder(section) != "saves"
}

fn file_is_allowed(file_name: &str, section: Option<&str>) -> bool {
    let lower = file_name.to_ascii_lowercase();
    match section_folder(section) {
        "shaderpacks" => lower.ends_with(".zip") || lower.ends_with(".disabled"),
        "resourcepacks" => lower.ends_with(".zip") || lower.ends_with(".disabled"),
        "saves" => true,
        _ => {
            lower.ends_with(".jar")
                || lower.ends_with(".disabled")
                || lower.ends_with(".jar.disabled")
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceModEntry {
    pub id: String,
    pub file_name: String,
    pub name: String,
    pub version: String,
    pub provider: String,
    pub enabled: bool,
    pub size_bytes: u64,
    pub modified_at: Option<u64>,
}

#[tauri::command]
pub fn list_instance_mods(
    instance_root: String,
    section: Option<String>,
) -> Result<Vec<InstanceModEntry>, String> {
    let mods_dir = PathBuf::from(instance_root)
        .join("minecraft")
        .join(section_folder(section.as_deref()));
    if !mods_dir.exists() {
        return Ok(Vec::new());
    }

    let mut rows: Vec<InstanceModEntry> = fs::read_dir(&mods_dir)
        .map_err(|err| {
            format!(
                "No se pudo leer carpeta de mods {}: {err}",
                mods_dir.display()
            )
        })?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| {
            let file_name = entry.file_name().to_string_lossy().to_string();
            let lower = file_name.to_lowercase();
            if !file_is_allowed(&file_name, section.as_deref()) {
                return None;
            }
            let metadata = entry.metadata().ok()?;
            let enabled = !lower.ends_with(".disabled");
            let base = file_name
                .trim_end_matches(".jar.disabled")
                .trim_end_matches(".disabled")
                .trim_end_matches(".jar")
                .to_string();
            let (name, version) = split_name_and_version(&base);
            let provider = detect_provider(&lower);
            let modified_at = metadata
                .modified()
                .ok()
                .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs());

            Some(InstanceModEntry {
                id: format!("{}-{}", base, metadata.len()),
                file_name,
                name,
                version,
                provider,
                enabled,
                size_bytes: metadata.len(),
                modified_at,
            })
        })
        .collect();

    rows.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(rows)
}

#[tauri::command]
pub fn set_instance_mod_enabled(
    instance_root: String,
    file_name: String,
    enabled: bool,
    section: Option<String>,
) -> Result<(), String> {
    if !section_allows_disable(section.as_deref()) {
        return Ok(());
    }
    let mods_dir = PathBuf::from(instance_root)
        .join("minecraft")
        .join(section_folder(section.as_deref()));
    let source_path = mods_dir.join(&file_name);
    if !source_path.exists() {
        return Err(format!(
            "No existe el mod seleccionado: {}",
            source_path.display()
        ));
    }

    let lower = file_name.to_lowercase();
    if enabled {
        if !lower.ends_with(".disabled") {
            return Ok(());
        }
        let next_name = if lower.ends_with(".jar.disabled") {
            file_name.trim_end_matches(".disabled").to_string()
        } else {
            file_name.trim_end_matches(".disabled").to_string()
        };
        let target_path = mods_dir.join(next_name);
        fs::rename(&source_path, target_path)
            .map_err(|err| format!("No se pudo activar mod: {err}"))?;
        return Ok(());
    }

    if lower.ends_with(".disabled") {
        return Ok(());
    }

    let target_path = mods_dir.join(format!("{file_name}.disabled"));
    fs::rename(&source_path, target_path)
        .map_err(|err| format!("No se pudo desactivar mod: {err}"))?;
    Ok(())
}

#[tauri::command]
pub fn replace_instance_mod_file(
    instance_root: String,
    current_file_name: String,
    download_url: String,
    new_file_name: String,
    section: Option<String>,
) -> Result<(), String> {
    let mods_dir = PathBuf::from(instance_root)
        .join("minecraft")
        .join(section_folder(section.as_deref()));
    fs::create_dir_all(&mods_dir)
        .map_err(|err| format!("No se pudo preparar carpeta de mods: {err}"))?;

    let response = reqwest::blocking::get(&download_url)
        .map_err(|err| format!("No se pudo descargar versión seleccionada: {err}"))?;
    let bytes = response
        .bytes()
        .map_err(|err| format!("No se pudo leer descarga de versión: {err}"))?;

    let new_target = mods_dir.join(&new_file_name);
    fs::write(&new_target, &bytes)
        .map_err(|err| format!("No se pudo guardar la nueva versión: {err}"))?;

    let old_target = mods_dir.join(&current_file_name);
    if old_target.exists() {
        let _ = fs::remove_file(old_target);
    }

    Ok(())
}

#[tauri::command]
pub fn install_catalog_mod_file(
    instance_root: String,
    download_url: String,
    file_name: String,
    replace_existing: bool,
    section: Option<String>,
) -> Result<(), String> {
    let mods_dir = PathBuf::from(instance_root)
        .join("minecraft")
        .join(section_folder(section.as_deref()));
    fs::create_dir_all(&mods_dir)
        .map_err(|err| format!("No se pudo preparar carpeta de mods: {err}"))?;

    let response = reqwest::blocking::get(&download_url)
        .map_err(|err| format!("No se pudo descargar mod seleccionado: {err}"))?;
    let bytes = response
        .bytes()
        .map_err(|err| format!("No se pudo leer descarga del mod: {err}"))?;

    let safe_name = file_name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    let target_name = match section_folder(section.as_deref()) {
        "shaderpacks" | "resourcepacks" => {
            if safe_name.to_ascii_lowercase().ends_with(".zip") {
                safe_name
            } else {
                format!("{safe_name}.zip")
            }
        }
        "saves" => safe_name,
        _ => {
            if safe_name.to_ascii_lowercase().ends_with(".jar") {
                safe_name
            } else {
                format!("{safe_name}.jar")
            }
        }
    };
    let target_path = mods_dir.join(target_name);
    if target_path.exists() && !replace_existing {
        return Ok(());
    }

    fs::write(&target_path, &bytes)
        .map_err(|err| format!("No se pudo guardar mod descargado: {err}"))?;

    Ok(())
}

fn split_name_and_version(base: &str) -> (String, String) {
    let mut pieces = base.rsplitn(2, '-');
    let version_candidate = pieces.next().unwrap_or_default().trim();
    let name_candidate = pieces.next().unwrap_or(base).trim();
    let looks_like_version = version_candidate.chars().any(|ch| ch.is_ascii_digit());
    if looks_like_version && !name_candidate.is_empty() {
        (
            name_candidate.replace(['_', '.'], " "),
            version_candidate.to_string(),
        )
    } else {
        (base.replace(['_', '.'], " "), "-".to_string())
    }
}

fn detect_provider(file_name: &str) -> String {
    if file_name.contains("modrinth") || file_name.contains("mrpack") {
        return "Modrinth".to_string();
    }
    if file_name.contains("curse") || file_name.contains("cf") {
        return "CurseForge".to_string();
    }
    if file_name.contains("http") || file_name.contains("url") || file_name.contains("external") {
        return "Externo".to_string();
    }
    "Local".to_string()
}
