use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::{path::BaseDirectory, Manager};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateInstancePayload {
    name: String,
    group: String,
    minecraft_version: String,
    loader: String,
    loader_version: String,
    ram_mb: u32,
    java_args: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateInstanceResult {
    id: String,
    name: String,
    group: String,
    launcher_root: String,
    instance_root: String,
    minecraft_path: String,
    logs: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstanceMetadata {
    name: String,
    group: String,
    minecraft_version: String,
    loader: String,
    loader_version: String,
    ram_mb: u32,
    java_args: Vec<String>,
    last_used: Option<String>,
    internal_uuid: String,
}

#[tauri::command]
fn create_instance(
    app: tauri::AppHandle,
    payload: CreateInstancePayload,
) -> Result<CreateInstanceResult, String> {
    let mut logs: Vec<String> = Vec::new();

    if payload.name.trim().is_empty() {
        return Err("El nombre de la instancia es obligatorio.".to_string());
    }

    if payload.minecraft_version.trim().is_empty() {
        return Err("La versión de Minecraft es obligatoria.".to_string());
    }

    let launcher_root = resolve_launcher_root(&app)?;
    logs.push(format!("Base launcher: {}", launcher_root.display()));

    create_launcher_directories(&launcher_root, &mut logs)?;
    prepare_embedded_java(&launcher_root, &mut logs)?;

    logs.push(format!(
        "Validando versión seleccionada: {}",
        payload.minecraft_version
    ));
    logs.push(
        "Descargando version_manifest.json (paso preparado para integración real).".to_string(),
    );
    logs.push(
        "Descargando client.jar y version.json (paso preparado para integración real).".to_string(),
    );
    logs.push("Descargando libraries/assets (paso preparado para integración real).".to_string());
    if payload.loader != "vanilla" {
        logs.push(format!(
            "Instalando loader {} {} (paso preparado para integración real).",
            payload.loader, payload.loader_version
        ));
    }

    let sanitized_name = sanitize_path_segment(&payload.name);
    let instance_root = launcher_root.join("instances").join(&sanitized_name);
    let minecraft_root = instance_root.join("minecraft");

    fs::create_dir_all(&instance_root).map_err(|err| err.to_string())?;
    logs.push(format!("Creada carpeta base: {}", instance_root.display()));

    let structure_dirs = [
        instance_root.join("logs"),
        instance_root.join("mods"),
        instance_root.join("config"),
        instance_root.join("resourcepacks"),
        instance_root.join("shaderpacks"),
        instance_root.join("saves"),
        minecraft_root.join("assets"),
        minecraft_root.join("libraries"),
        minecraft_root
            .join("versions")
            .join(&payload.minecraft_version),
        minecraft_root.join("mods"),
        minecraft_root.join("config"),
        minecraft_root.join("logs"),
        minecraft_root.join("crash-reports"),
        minecraft_root.join("saves"),
    ];

    for dir in structure_dirs {
        fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    }
    logs.push("Estructura interna de instancia y .minecraft creada.".to_string());

    let version_file_base = minecraft_root
        .join("versions")
        .join(&payload.minecraft_version);
    let jar_path = version_file_base.join(format!("{}.jar", payload.minecraft_version));
    let json_path = version_file_base.join(format!("{}.json", payload.minecraft_version));

    write_placeholder_file(&jar_path, "placeholder minecraft jar")?;
    write_placeholder_file(
        &json_path,
        &format!(
            "{{\"id\":\"{}\",\"type\":\"release\"}}",
            payload.minecraft_version
        ),
    )?;

    logs.push(
        "Classpath generado (placeholder para implementación del launcher runtime).".to_string(),
    );

    let internal_uuid = uuid::Uuid::new_v4().to_string();
    let metadata = InstanceMetadata {
        name: payload.name,
        group: payload.group,
        minecraft_version: payload.minecraft_version,
        loader: payload.loader,
        loader_version: payload.loader_version,
        ram_mb: payload.ram_mb,
        java_args: payload.java_args,
        last_used: None,
        internal_uuid: internal_uuid.clone(),
    };

    let metadata_path = instance_root.join(".instance.json");
    let metadata_content =
        serde_json::to_string_pretty(&metadata).map_err(|err| err.to_string())?;
    fs::write(&metadata_path, metadata_content).map_err(|err| err.to_string())?;
    logs.push(format!("Metadata guardada en {}", metadata_path.display()));

    Ok(CreateInstanceResult {
        id: internal_uuid,
        name: metadata.name,
        group: metadata.group,
        launcher_root: launcher_root.display().to_string(),
        instance_root: instance_root.display().to_string(),
        minecraft_path: minecraft_root.display().to_string(),
        logs,
    })
}

fn resolve_launcher_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .resolve("InterfaceLauncher", BaseDirectory::AppData)
        .map_err(|err| err.to_string())
}

fn create_launcher_directories(root: &Path, logs: &mut Vec<String>) -> Result<(), String> {
    let dirs = [
        root.join("runtime/jre/bin"),
        root.join("runtime/jre/lib"),
        root.join("runtime/jre/conf"),
        root.join("instances"),
        root.join("assets"),
        root.join("cache"),
        root.join("logs"),
        root.join("config"),
        root.join("versions"),
    ];

    for dir in dirs {
        fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    }

    let launcher_config = root.join("config/launcher.json");
    if !launcher_config.exists() {
        fs::write(
      &launcher_config,
      "{\n  \"defaultPage\": \"Mis Modpacks\",\n  \"javaPath\": \"runtime/jre/bin/java.exe\"\n}\n",
    )
    .map_err(|err| err.to_string())?;
    }

    let accounts_config = root.join("config/accounts.json");
    if !accounts_config.exists() {
        fs::write(&accounts_config, "[]\n").map_err(|err| err.to_string())?;
    }

    logs.push("Estructura global del launcher verificada/creada.".to_string());
    Ok(())
}

fn prepare_embedded_java(root: &Path, logs: &mut Vec<String>) -> Result<(), String> {
    let java_binary = root.join("runtime/jre/bin/java.exe");
    if !java_binary.exists() {
        write_placeholder_file(&java_binary, "embedded jre placeholder")?;
        logs.push("Java embebido preparado en runtime/jre (placeholder inicial).".to_string());
    } else {
        logs.push("Java embebido ya existente en runtime/jre.".to_string());
    }

    logs.push("Arquitectura detectada: x64 (placeholder).".to_string());
    logs.push("Verificación de checksum de JRE marcada para flujo real de descarga.".to_string());

    Ok(())
}

fn write_placeholder_file(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    fs::write(path, content).map_err(|err| err.to_string())
}

fn sanitize_path_segment(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == ' ' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim()
        .replace(' ', "-")
        .to_lowercase();

    if sanitized.is_empty() {
        "instance".to_string()
    } else {
        sanitized
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .invoke_handler(tauri::generate_handler![create_instance])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
