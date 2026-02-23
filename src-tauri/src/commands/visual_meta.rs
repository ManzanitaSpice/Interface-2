use serde::{Deserialize, Serialize};
use std::{fs, path::{Path, PathBuf}, time::{SystemTime, UNIX_EPOCH}};

const VISUAL_META_FILE: &str = ".interface-visual.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceVisualMeta {
    pub media_data_url: Option<String>,
    pub media_path: Option<String>,
    pub media_mime: Option<String>,
    pub minecraft_version: Option<String>,
    pub loader: Option<String>,
}

#[tauri::command]
pub fn save_instance_visual_media(
    instance_root: String,
    file_name: String,
    bytes: Vec<u8>,
    previous_media_path: Option<String>,
) -> Result<String, String> {
    if bytes.is_empty() {
        return Err("El archivo visual está vacío.".to_string());
    }
    let safe_name = sanitize_file_name(&file_name);
    let extension = Path::new(&safe_name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("bin");
    let media_dir = PathBuf::from(&instance_root).join(".interface-media");
    fs::create_dir_all(&media_dir).map_err(|err| format!("No se pudo preparar carpeta media: {err}"))?;

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| format!("Reloj del sistema inválido: {err}"))?
        .as_millis();
    let target = media_dir.join(format!("instance-media-{stamp}.{extension}"));
    fs::write(&target, bytes).map_err(|err| format!("No se pudo guardar archivo visual: {err}"))?;

    if let Some(previous) = previous_media_path {
        let previous_path = PathBuf::from(previous);
        if previous_path.exists()
            && previous_path.starts_with(&media_dir)
            && previous_path != target
        {
            let _ = fs::remove_file(previous_path);
        }
    }

    Ok(target.display().to_string())
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
    let mut parsed = serde_json::from_str::<InstanceVisualMeta>(&content)
        .map_err(|err| format!("Metadata visual inválida: {err}"))?;
    if let Some(path) = parsed.media_path.as_ref() {
        if !Path::new(path).exists() {
            parsed.media_path = None;
        }
    }
    Ok(Some(parsed))
}

fn sanitize_file_name(file_name: &str) -> String {
    let trimmed = file_name.trim();
    if trimmed.is_empty() {
        return "instance-media.bin".to_string();
    }
    trimmed
        .chars()
        .map(|char| if char.is_ascii_alphanumeric() || char == '.' || char == '-' || char == '_' { char } else { '_' })
        .collect::<String>()
}
