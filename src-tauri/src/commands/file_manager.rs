use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkinSummary {
    pub id: String,
    pub name: String,
    pub updated_at: String,
}

fn root_dir() -> Result<PathBuf, String> {
    let base = if cfg!(target_os = "windows") {
        std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    } else {
        std::env::var("HOME")
            .map(|home| PathBuf::from(home).join(".local/share"))
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    };
    let dir = base.join("InterfaceLauncher").join("assets").join("skins");
    fs::create_dir_all(&dir).map_err(|err| format!("No se pudo crear skin-storage: {err}"))?;
    Ok(dir)
}

fn account_dir(account_id: &str) -> Result<PathBuf, String> {
    let dir = root_dir()?.join(account_id);
    fs::create_dir_all(&dir).map_err(|err| format!("No se pudo crear carpeta de cuenta: {err}"))?;
    Ok(dir)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[tauri::command]
pub fn list_skins(account_id: String) -> Result<Vec<SkinSummary>, String> {
    let dir = account_dir(&account_id)?;
    let mut skins = Vec::new();
    let entries = fs::read_dir(&dir).map_err(|err| format!("No se pudo leer skins: {err}"))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|v| v.to_str()) != Some("png") {
            continue;
        }
        let stem = path.file_stem().and_then(|v| v.to_str()).unwrap_or("skin");
        let parts: Vec<&str> = stem.splitn(2, "__").collect();
        let (id, name) = if parts.len() == 2 {
            (parts[0].to_string(), parts[1].replace('_', " "))
        } else {
            (stem.to_string(), stem.to_string())
        };

        let updated_at = fs::metadata(&path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .map(|t| {
                let datetime: chrono::DateTime<chrono::Local> = t.into();
                datetime.format("%d/%m/%Y %H:%M").to_string()
            })
            .unwrap_or_else(|| "-".into());

        skins.push(SkinSummary {
            id,
            name,
            updated_at,
        });
    }

    skins.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(skins)
}

#[tauri::command]
pub fn import_skin(
    account_id: String,
    name: String,
    bytes: Vec<u8>,
) -> Result<SkinSummary, String> {
    crate::commands::validator::validate_skin_png(&bytes)?;
    let optimized = crate::commands::skin_processor::optimize_skin_png(bytes)?;

    let id = Uuid::new_v4().to_string();
    let safe_name = name
        .trim()
        .replace(' ', "_")
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '-')
        .collect::<String>();
    let final_name = if safe_name.is_empty() {
        "skin".to_string()
    } else {
        safe_name
    };

    let path = account_dir(&account_id)?.join(format!("{id}__{final_name}.png"));
    fs::write(&path, optimized).map_err(|err| format!("No se pudo guardar la skin: {err}"))?;

    Ok(SkinSummary {
        id,
        name: final_name.replace('_', " "),
        updated_at: chrono::Local::now().format("%d/%m/%Y %H:%M").to_string(),
    })
}

#[tauri::command]
pub fn delete_skin(account_id: String, skin_id: String) -> Result<(), String> {
    let dir = account_dir(&account_id)?;
    let entries = fs::read_dir(&dir).map_err(|err| format!("No se pudo leer carpeta: {err}"))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|v| v.to_str()) else {
            continue;
        };
        if file_name.starts_with(&skin_id) && file_name.ends_with(".png") {
            fs::remove_file(path).map_err(|err| format!("No se pudo eliminar skin: {err}"))?;
            return Ok(());
        }
    }
    Err("No se encontró la skin".into())
}

#[tauri::command]
pub fn load_skin_binary(account_id: String, skin_id: String) -> Result<Vec<u8>, String> {
    let dir = account_dir(&account_id)?;
    let entries = fs::read_dir(&dir).map_err(|err| format!("No se pudo leer carpeta: {err}"))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|v| v.to_str()) else {
            continue;
        };
        if file_name.starts_with(&skin_id) && file_name.ends_with(".png") {
            return fs::read(path).map_err(|err| format!("No se pudo abrir skin: {err}"));
        }
    }
    Err("No se encontró la skin".into())
}

#[tauri::command]
pub fn save_skin_binary(
    account_id: String,
    skin_id: String,
    bytes: Vec<u8>,
) -> Result<SkinSummary, String> {
    crate::commands::validator::validate_skin_png(&bytes)?;
    let optimized = crate::commands::skin_processor::optimize_skin_png(bytes)?;

    let dir = account_dir(&account_id)?;
    let entries = fs::read_dir(&dir).map_err(|err| format!("No se pudo leer carpeta: {err}"))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|v| v.to_str()) else {
            continue;
        };
        if file_name.starts_with(&skin_id) && file_name.ends_with(".png") {
            let current =
                fs::read(&path).map_err(|err| format!("No se pudo leer skin existente: {err}"))?;
            let incoming_hash = sha256_hex(&optimized);
            let current_hash = sha256_hex(&current);
            if incoming_hash != current_hash {
                fs::write(&path, optimized)
                    .map_err(|err| format!("No se pudo guardar cambios: {err}"))?;
            }
            let stem = path.file_stem().and_then(|v| v.to_str()).unwrap_or("skin");
            let name = stem
                .splitn(2, "__")
                .nth(1)
                .unwrap_or("skin")
                .replace('_', " ");
            return Ok(SkinSummary {
                id: skin_id,
                name,
                updated_at: chrono::Local::now().format("%d/%m/%Y %H:%M").to_string(),
            });
        }
    }

    Err("No se encontró la skin".into())
}
