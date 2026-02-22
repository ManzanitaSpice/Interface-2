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
