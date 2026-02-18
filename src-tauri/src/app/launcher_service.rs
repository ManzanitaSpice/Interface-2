use std::fs;

use tauri::AppHandle;

use crate::{
    domain::{
        java::java_requirement::determine_required_java,
        models::{
            instance::{CreateInstancePayload, CreateInstanceResult, InstanceMetadata},
            java::JavaRuntime,
        },
    },
    infrastructure::filesystem::paths::resolve_launcher_root,
    services::{
        instance_builder::{build_instance_structure, persist_instance_metadata},
        java_installer::ensure_embedded_java,
    },
    shared::result::AppResult,
};

#[tauri::command]
pub fn create_instance(
    app: AppHandle,
    payload: CreateInstancePayload,
) -> Result<CreateInstanceResult, String> {
    create_instance_impl(app, payload)
}

fn create_instance_impl(
    app: AppHandle,
    payload: CreateInstancePayload,
) -> AppResult<CreateInstanceResult> {
    let mut logs: Vec<String> = Vec::new();

    validate_payload(&payload)?;

    let launcher_root = resolve_launcher_root(&app)?;
    logs.push(format!("Base launcher: {}", launcher_root.display()));

    crate::infrastructure::filesystem::directories::create_launcher_directories(
        &launcher_root,
        &mut logs,
    )?;

    let required_java = determine_required_java(&payload.minecraft_version, &payload.loader)?;
    logs.push(format!(
        "Java requerido detectado para MC {} + loader {}: Java {}.",
        payload.minecraft_version,
        payload.loader,
        required_java.major()
    ));

    let java_exec = ensure_embedded_java(&launcher_root, required_java, &mut logs)?;

    log_download_steps(&payload, &mut logs);

    let sanitized_name =
        crate::infrastructure::filesystem::paths::sanitize_path_segment(&payload.name);
    let instance_root = launcher_root.join("instances").join(&sanitized_name);
    let minecraft_root = instance_root.join("minecraft");

    fs::create_dir_all(&instance_root)?;
    logs.push(format!("Creada carpeta base: {}", instance_root.display()));

    build_instance_structure(
        &instance_root,
        &minecraft_root,
        &payload.minecraft_version,
        &mut logs,
    )?;

    let internal_uuid = uuid::Uuid::new_v4().to_string();
    let metadata = InstanceMetadata {
        name: payload.name,
        group: payload.group,
        minecraft_version: payload.minecraft_version,
        loader: payload.loader,
        loader_version: payload.loader_version,
        ram_mb: payload.ram_mb,
        java_args: payload.java_args,
        java_path: java_exec.display().to_string(),
        java_runtime: runtime_name(required_java).to_string(),
        last_used: None,
        internal_uuid: internal_uuid.clone(),
    };

    persist_instance_metadata(&instance_root, &metadata, &mut logs)?;

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

fn validate_payload(payload: &CreateInstancePayload) -> AppResult<()> {
    if payload.name.trim().is_empty() {
        return Err("El nombre de la instancia es obligatorio.".to_string());
    }

    if payload.minecraft_version.trim().is_empty() {
        return Err("La versión de Minecraft es obligatoria.".to_string());
    }

    Ok(())
}

fn runtime_name(runtime: JavaRuntime) -> &'static str {
    runtime.as_dir_name()
}

fn log_download_steps(payload: &CreateInstancePayload, logs: &mut Vec<String>) {
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
}
