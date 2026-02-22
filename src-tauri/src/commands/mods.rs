use serde::Serialize;
use std::{fs, path::PathBuf, time::UNIX_EPOCH};

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
pub fn list_instance_mods(instance_root: String) -> Result<Vec<InstanceModEntry>, String> {
    let mods_dir = PathBuf::from(instance_root).join("minecraft").join("mods");
    if !mods_dir.exists() {
        return Ok(Vec::new());
    }

    let mut rows: Vec<InstanceModEntry> = fs::read_dir(&mods_dir)
        .map_err(|err| format!("No se pudo leer carpeta de mods {}: {err}", mods_dir.display()))?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| {
            let file_name = entry.file_name().to_string_lossy().to_string();
            let lower = file_name.to_lowercase();
            if !(lower.ends_with(".jar") || lower.ends_with(".disabled") || lower.ends_with(".jar.disabled")) {
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
pub fn set_instance_mod_enabled(instance_root: String, file_name: String, enabled: bool) -> Result<(), String> {
    let mods_dir = PathBuf::from(instance_root).join("minecraft").join("mods");
    let source_path = mods_dir.join(&file_name);
    if !source_path.exists() {
        return Err(format!("No existe el mod seleccionado: {}", source_path.display()));
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
) -> Result<(), String> {
    let mods_dir = PathBuf::from(instance_root).join("minecraft").join("mods");
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

fn split_name_and_version(base: &str) -> (String, String) {
    let mut pieces = base.rsplitn(2, '-');
    let version_candidate = pieces.next().unwrap_or_default().trim();
    let name_candidate = pieces.next().unwrap_or(base).trim();
    let looks_like_version = version_candidate.chars().any(|ch| ch.is_ascii_digit());
    if looks_like_version && !name_candidate.is_empty() {
        (name_candidate.replace(['_', '.'], " "), version_candidate.to_string())
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
    "Desconocido".to_string()
}
