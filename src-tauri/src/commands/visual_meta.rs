use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

const VISUAL_META_FILE: &str = ".interface-visual.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceVisualMeta {
    pub media_data_url: Option<String>,
    pub media_mime: Option<String>,
    pub minecraft_version: Option<String>,
    pub loader: Option<String>,
}

#[tauri::command]
pub fn save_instance_visual_meta(instance_root: String, meta: InstanceVisualMeta) -> Result<(), String> {
    let path = PathBuf::from(instance_root).join(VISUAL_META_FILE);
    let payload = serde_json::to_string_pretty(&meta).map_err(|err| format!("No se pudo serializar visual meta: {err}"))?;
    fs::write(path, payload).map_err(|err| format!("No se pudo guardar metadata visual: {err}"))
}

#[tauri::command]
pub fn load_instance_visual_meta(instance_root: String) -> Result<Option<InstanceVisualMeta>, String> {
    let path = PathBuf::from(instance_root).join(VISUAL_META_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path).map_err(|err| format!("No se pudo leer metadata visual: {err}"))?;
    let parsed = serde_json::from_str::<InstanceVisualMeta>(&content)
        .map_err(|err| format!("Metadata visual inválida: {err}"))?;
    Ok(Some(parsed))
}
