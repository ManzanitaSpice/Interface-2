use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, OnceLock,
    },
};

use tauri::{AppHandle, Emitter};
use uuid::Uuid;

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedInstance {
    id: String,
    name: String,
    source_launcher: String,
    source_path: String,
    minecraft_version: String,
    loader: String,
    loader_version: String,
    format: String,
    icon_path: Option<String>,
    mods_count: Option<u32>,
    size_mb: Option<u64>,
    last_played: Option<String>,
    importable: bool,
    import_warnings: Vec<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRequest {
    detected_instance_id: String,
    target_name: String,
    target_group: String,
    ram_mb: u32,
    copy_mods: bool,
    copy_worlds: bool,
    copy_resourcepacks: bool,
    copy_screenshots: bool,
    copy_logs: bool,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanProgressEvent {
    stage: String,
    message: String,
    found_so_far: usize,
    current_path: String,
}

static CANCEL_IMPORT: OnceLock<Arc<AtomicBool>> = OnceLock::new();

fn known_paths() -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    if let Ok(appdata) = std::env::var("APPDATA") {
        out.push((
            "CurseForge".to_string(),
            PathBuf::from(&appdata).join("CurseForge/Minecraft/Instances"),
        ));
        out.push((
            "Modrinth".to_string(),
            PathBuf::from(&appdata).join("com.modrinth.theseus/profiles"),
        ));
        out.push((
            "Prism".to_string(),
            PathBuf::from(&appdata).join("PrismLauncher/instances"),
        ));
    }
    if let Ok(home) = std::env::var("HOME") {
        out.push((
            "Prism".to_string(),
            PathBuf::from(&home).join(".local/share/PrismLauncher/instances"),
        ));
        out.push((
            "Mojang Official".to_string(),
            PathBuf::from(&home).join(".minecraft"),
        ));
    }
    out
}

fn detect_dir(path: &Path, launcher: &str) -> Option<DetectedInstance> {
    if !path.is_dir() {
        return None;
    }
    let name = path.file_name()?.to_string_lossy().to_string();
    let importable = path.join("minecraftinstance.json").exists()
        || path.join("profile.json").exists()
        || path.join("mmc-pack.json").exists()
        || path.join("instance.cfg").exists()
        || path.join(".minecraft").exists();

    Some(DetectedInstance {
        id: Uuid::new_v4().to_string(),
        name,
        source_launcher: launcher.to_string(),
        source_path: path.display().to_string(),
        minecraft_version: "desconocida".to_string(),
        loader: "desconocido".to_string(),
        loader_version: "-".to_string(),
        format: "directory".to_string(),
        icon_path: None,
        mods_count: None,
        size_mb: None,
        last_played: None,
        importable,
        import_warnings: if importable {
            vec![]
        } else {
            vec!["No se detectaron archivos de formato conocido".to_string()]
        },
    })
}

#[tauri::command]
pub fn detect_external_instances(app: AppHandle) -> Result<Vec<DetectedInstance>, String> {
    CANCEL_IMPORT
        .get_or_init(|| Arc::new(AtomicBool::new(false)))
        .store(false, Ordering::Relaxed);
    let mut found = Vec::new();

    for (launcher, root) in known_paths() {
        let _ = app.emit(
            "import_scan_progress",
            ScanProgressEvent {
                stage: format!("scanning_{}", launcher.to_lowercase().replace(' ', "_")),
                message: format!("Buscando en {launcher}..."),
                found_so_far: found.len(),
                current_path: root.display().to_string(),
            },
        );

        if !root.exists() {
            continue;
        }
        let entries =
            fs::read_dir(&root).map_err(|e| format!("No se pudo leer {}: {e}", root.display()))?;
        for entry in entries {
            if CANCEL_IMPORT
                .get()
                .is_some_and(|flag| flag.load(Ordering::Relaxed))
            {
                return Ok(found);
            }
            let entry = entry.map_err(|e| format!("No se pudo leer entrada: {e}"))?;
            if let Some(instance) = detect_dir(&entry.path(), &launcher) {
                let _ = app.emit("import_scan_result", instance.clone());
                found.push(instance);
            }
        }
    }

    Ok(found)
}

#[tauri::command]
pub fn import_specific(path: String) -> Result<Vec<DetectedInstance>, String> {
    let p = PathBuf::from(path);
    if p.is_file() {
        let name = p
            .file_name()
            .map(|v| v.to_string_lossy().to_string())
            .unwrap_or_else(|| "archivo".to_string());
        return Ok(vec![DetectedInstance {
            id: Uuid::new_v4().to_string(),
            name,
            source_launcher: "Archivo".to_string(),
            source_path: p.display().to_string(),
            minecraft_version: "desconocida".to_string(),
            loader: "desconocido".to_string(),
            loader_version: "-".to_string(),
            format: p
                .extension()
                .map(|v| v.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            icon_path: None,
            mods_count: None,
            size_mb: None,
            last_played: None,
            importable: true,
            import_warnings: vec![],
        }]);
    }

    if p.is_dir() {
        let mut out = Vec::new();
        for entry in
            fs::read_dir(&p).map_err(|e| format!("No se pudo leer {}: {e}", p.display()))?
        {
            let entry = entry.map_err(|e| format!("No se pudo leer entrada: {e}"))?;
            if let Some(instance) = detect_dir(&entry.path(), "Manual") {
                out.push(instance);
            }
        }
        return Ok(out);
    }

    Ok(Vec::new())
}

#[tauri::command]
pub fn execute_import(app: AppHandle, requests: Vec<ImportRequest>) -> Result<(), String> {
    for (index, req) in requests.iter().enumerate() {
        let _ = app.emit(
            "import_execution_progress",
            serde_json::json!({
                "instanceId": req.detected_instance_id,
                "instanceName": req.target_name,
                "step": "creating_instance",
                "stepIndex": 1,
                "totalSteps": 1,
                "completed": index + 1,
                "total": requests.len(),
                "message": format!("Preparando importación de {}", req.target_name)
            }),
        );
        let _ = app.emit(
            "import_instance_completed",
            serde_json::json!({
                "success": true,
                "instanceId": req.detected_instance_id,
                "error": serde_json::Value::Null
            }),
        );
    }
    Ok(())
}

#[tauri::command]
pub fn cancel_import() {
    if let Some(flag) = CANCEL_IMPORT.get() {
        flag.store(true, Ordering::Relaxed);
    }
}
