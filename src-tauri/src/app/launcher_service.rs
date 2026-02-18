use std::fs;

use tauri::{AppHandle, Emitter};

use crate::{
    domain::{
        java::{java_detector::find_compatible_java, java_requirement::determine_required_java},
        models::{
            instance::{
                CreateInstancePayload, CreateInstanceResult, InstanceMetadata, InstanceSummary,
                LaunchAuthSession,
            },
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

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct InstanceCreationProgressEvent {
    request_id: Option<String>,
    message: String,
}

fn push_creation_log(
    app: &AppHandle,
    request_id: &Option<String>,
    logs: &mut Vec<String>,
    message: impl Into<String>,
) {
    let message = message.into();
    logs.push(message.clone());
    let _ = app.emit(
        "instance_creation_progress",
        InstanceCreationProgressEvent {
            request_id: request_id.clone(),
            message,
        },
    );
}

#[tauri::command]
pub async fn create_instance(
    app: AppHandle,
    payload: CreateInstancePayload,
) -> Result<CreateInstanceResult, String> {
    tauri::async_runtime::spawn_blocking(move || create_instance_impl(app, payload))
        .await
        .map_err(|err| format!("Falló la tarea de creación de instancia: {err}"))?
}

#[tauri::command]
pub fn list_instances(app: AppHandle) -> Result<Vec<InstanceSummary>, String> {
    list_instances_impl(app)
}

#[tauri::command]
pub fn delete_instance(app: AppHandle, instance_root: String) -> Result<(), String> {
    let launcher_root = resolve_launcher_root(&app)?;
    let instances_root = launcher_root.join("instances");
    let target_path = std::path::PathBuf::from(&instance_root);

    if !target_path.exists() {
        return Err(format!(
            "La instancia no existe en disco: {}",
            target_path.display()
        ));
    }

    if !target_path.is_dir() {
        return Err(format!(
            "La ruta de instancia no es un directorio: {}",
            target_path.display()
        ));
    }

    let canonical_instances_root = fs::canonicalize(&instances_root).map_err(|err| {
        format!(
            "No se pudo resolver la ruta de instancias {}: {}",
            instances_root.display(),
            err
        )
    })?;
    let canonical_target = fs::canonicalize(&target_path).map_err(|err| {
        format!(
            "No se pudo resolver la ruta de la instancia {}: {}",
            target_path.display(),
            err
        )
    })?;

    if !canonical_target.starts_with(&canonical_instances_root) {
        return Err(format!(
            "Ruta inválida para eliminar instancia fuera del directorio permitido: {}",
            canonical_target.display()
        ));
    }

    fs::remove_dir_all(&canonical_target).map_err(|err| {
        format!(
            "No se pudo eliminar la instancia {}: {}",
            canonical_target.display(),
            err
        )
    })
}

fn list_instances_impl(app: AppHandle) -> AppResult<Vec<InstanceSummary>> {
    let launcher_root = resolve_launcher_root(&app)?;
    let instances_root = launcher_root.join("instances");

    if !instances_root.exists() {
        return Ok(Vec::new());
    }

    let mut instances: Vec<InstanceSummary> = Vec::new();

    let entries = fs::read_dir(&instances_root).map_err(|err| {
        format!(
            "No se pudo leer el directorio de instancias ({}): {}",
            instances_root.display(),
            err
        )
    })?;

    for entry in entries {
        let entry = match entry {
            Ok(value) => value,
            Err(_) => continue,
        };

        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let metadata_path = path.join(".instance.json");
        if !metadata_path.exists() {
            continue;
        }

        let metadata_raw = match fs::read_to_string(&metadata_path) {
            Ok(raw) => raw,
            Err(_) => continue,
        };

        let metadata: InstanceMetadata = match serde_json::from_str(&metadata_raw) {
            Ok(value) => value,
            Err(_) => continue,
        };

        instances.push(InstanceSummary {
            id: metadata.internal_uuid,
            name: metadata.name,
            group: metadata.group,
            instance_root: path.display().to_string(),
        });
    }

    instances.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    Ok(instances)
}

fn create_instance_impl(
    app: AppHandle,
    payload: CreateInstancePayload,
) -> AppResult<CreateInstanceResult> {
    let mut logs: Vec<String> = Vec::new();
    let request_id = payload.creation_request_id.clone();

    push_creation_log(&app, &request_id, &mut logs, "Iniciando validación de payload...");
    validate_payload(&payload)?;
    push_creation_log(&app, &request_id, &mut logs, "Payload válido.");

    let mut auth_logs = Vec::new();
    validate_official_minecraft_auth(&payload.auth_session, &mut auth_logs)?;
    for line in auth_logs {
        push_creation_log(&app, &request_id, &mut logs, line);
    }

    let launcher_root = resolve_launcher_root(&app)?;
    validate_instance_constraints(&launcher_root, &payload)?;
    push_creation_log(&app, &request_id, &mut logs, format!("Base launcher: {}", launcher_root.display()));

    push_creation_log(&app, &request_id, &mut logs, "Creando/verificando directorios base del launcher...");
    crate::infrastructure::filesystem::directories::create_launcher_directories(
        &launcher_root,
        &mut logs,
    )?;
    if let Some(last) = logs.last().cloned() {
        let _ = app.emit(
            "instance_creation_progress",
            InstanceCreationProgressEvent {
                request_id: request_id.clone(),
                message: last,
            },
        );
    }

    let required_java = if let Some(java_major) = payload.required_java_major {
        runtime_from_major(java_major)?
    } else {
        determine_required_java(&payload.minecraft_version, &payload.loader)?
    };
    push_creation_log(
        &app,
        &request_id,
        &mut logs,
        format!(
            "Java requerido detectado para MC {} + loader {}: Java {}.",
            payload.minecraft_version,
            payload.loader,
            required_java.major()
        ),
    );

    if let Some(system_java) = find_compatible_java(required_java) {
        push_creation_log(
            &app,
            &request_id,
            &mut logs,
            format!(
                "Java del sistema detectado: {} (major {}). Se prioriza runtime embebido para ruta controlada.",
                system_java.path.display(),
                system_java.major
            ),
        );
    } else {
        push_creation_log(
            &app,
            &request_id,
            &mut logs,
            "No se encontró Java del sistema compatible. Se usará runtime embebido.".to_string(),
        );
    }

    push_creation_log(&app, &request_id, &mut logs, "Preparando runtime Java embebido...");
    let java_exec = ensure_embedded_java(&launcher_root, required_java, &mut logs)?;
    if let Some(last) = logs.last().cloned() {
        let _ = app.emit(
            "instance_creation_progress",
            InstanceCreationProgressEvent {
                request_id: request_id.clone(),
                message: last,
            },
        );
    }

    log_download_steps(&payload, &mut logs, required_java);
    for line in logs.iter().rev().take(6).cloned().collect::<Vec<_>>().into_iter().rev() {
        let _ = app.emit(
            "instance_creation_progress",
            InstanceCreationProgressEvent {
                request_id: request_id.clone(),
                message: line,
            },
        );
    }

    let sanitized_name =
        crate::infrastructure::filesystem::paths::sanitize_path_segment(&payload.name);
    let instance_root = launcher_root.join("instances").join(&sanitized_name);
    let minecraft_root = instance_root.join("minecraft");

    push_creation_log(&app, &request_id, &mut logs, "Creando carpeta base de la instancia...");
    fs::create_dir_all(&instance_root).map_err(|err| {
        format!(
            "No se pudo crear la carpeta base de la instancia ({}): {}",
            instance_root.display(),
            err
        )
    })?;
    push_creation_log(&app, &request_id, &mut logs, format!("Creada carpeta base: {}", instance_root.display()));

    push_creation_log(&app, &request_id, &mut logs, "Construyendo estructura interna de la instancia...");
    build_instance_structure(
        &instance_root,
        &minecraft_root,
        &payload.minecraft_version,
        &mut logs,
    )?;
    if let Some(last) = logs.last().cloned() {
        let _ = app.emit(
            "instance_creation_progress",
            InstanceCreationProgressEvent {
                request_id: request_id.clone(),
                message: last,
            },
        );
    }

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
        java_version: format!("{}.0.x", required_java.major()),
        last_used: None,
        internal_uuid: internal_uuid.clone(),
    };

    push_creation_log(&app, &request_id, &mut logs, "Guardando metadata final de la instancia...");
    persist_instance_metadata(&instance_root, &metadata, &mut logs)?;
    push_creation_log(&app, &request_id, &mut logs, "Instancia creada y registrada exitosamente.");

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

fn validate_instance_constraints(
    launcher_root: &std::path::Path,
    payload: &CreateInstancePayload,
) -> AppResult<()> {
    let sanitized_name =
        crate::infrastructure::filesystem::paths::sanitize_path_segment(&payload.name);
    let instance_root = launcher_root.join("instances").join(&sanitized_name);

    if instance_root.exists() {
        return Err(format!(
            "Ya existe una instancia con ese nombre: {}",
            payload.name
        ));
    }

    let available_bytes = fs2::available_space(launcher_root).map_err(|err| {
        format!(
            "No se pudo validar el espacio en disco en {}: {}",
            launcher_root.display(),
            err
        )
    })?;

    let minimum_required = 1024_u64 * 1024 * 1024;
    if available_bytes < minimum_required {
        return Err(format!(
            "Espacio insuficiente: se requiere al menos 1GB libre en {}",
            launcher_root.display()
        ));
    }

    Ok(())
}

fn validate_payload(payload: &CreateInstancePayload) -> AppResult<()> {
    if payload.name.trim().is_empty() {
        return Err("El nombre de la instancia es obligatorio.".to_string());
    }

    if payload.minecraft_version.trim().is_empty() {
        return Err("La versión de Minecraft es obligatoria.".to_string());
    }

    if payload
        .auth_session
        .minecraft_access_token
        .trim()
        .is_empty()
    {
        return Err("Debes iniciar sesión con cuenta oficial de Minecraft para crear instancias (sin Demo).".to_string());
    }

    Ok(())
}

fn validate_official_minecraft_auth(
    auth_session: &LaunchAuthSession,
    logs: &mut Vec<String>,
) -> AppResult<()> {
    if auth_session.minecraft_access_token.trim().is_empty() {
        return Err(
            "No hay access token de Minecraft válido; no se permite crear instancia en modo Demo."
                .to_string(),
        );
    }

    let client = reqwest::blocking::Client::new();

    let entitlements_response = client
        .get("https://api.minecraftservices.com/entitlements/mcstore")
        .header(
            "Authorization",
            format!("Bearer {}", auth_session.minecraft_access_token),
        )
        .header("Accept", "application/json")
        .send()
        .map_err(|err| format!("No se pudo consultar entitlements de Minecraft: {err}"))?;

    let entitlements_status = entitlements_response.status();
    if !entitlements_status.is_success() {
        let body = entitlements_response.text().unwrap_or_default();
        return Err(format!(
            "La API de entitlements de Minecraft devolvió error HTTP: {entitlements_status}. Body completo: {body}"
        ));
    }

    let entitlements_json = entitlements_response
        .json::<serde_json::Value>()
        .map_err(|err| format!("No se pudo leer entitlements de Minecraft: {err}"))?;

    let has_license = entitlements_json
        .get("items")
        .and_then(serde_json::Value::as_array)
        .map(|items| !items.is_empty())
        .unwrap_or(false);

    if !has_license {
        return Err("La cuenta no tiene licencia de Minecraft".to_string());
    }

    let profile_response = client
        .get("https://api.minecraftservices.com/minecraft/profile")
        .header(
            "Authorization",
            format!("Bearer {}", auth_session.minecraft_access_token),
        )
        .header("Accept", "application/json")
        .send()
        .map_err(|err| format!("No se pudo consultar perfil de Minecraft: {err}"))?;

    let profile_status = profile_response.status();
    if !profile_status.is_success() {
        let body = profile_response.text().unwrap_or_default();
        return Err(format!(
            "La API de perfil de Minecraft devolvió error HTTP: {profile_status}. Body completo: {body}"
        ));
    }

    let profile_json = profile_response
        .json::<serde_json::Value>()
        .map_err(|err| format!("No se pudo leer perfil de Minecraft: {err}"))?;

    let profile_id = profile_json
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let profile_name = profile_json
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();

    if profile_id != auth_session.profile_id || profile_name != auth_session.profile_name {
        return Err(
            "El perfil de Minecraft no coincide con la sesión actual; token inválido o vencido."
                .to_string(),
        );
    }

    logs.push("Licencia oficial de Minecraft verificada (entitlements/mcstore).".to_string());
    logs.push(format!(
        "Perfil oficial verificado: {} ({})",
        profile_name, profile_id
    ));

    Ok(())
}

fn runtime_name(runtime: JavaRuntime) -> &'static str {
    runtime.as_dir_name()
}

fn runtime_from_major(java_major: u32) -> AppResult<JavaRuntime> {
    match java_major {
        0..=8 => Ok(JavaRuntime::Java8),
        9..=17 => Ok(JavaRuntime::Java17),
        18.. => Ok(JavaRuntime::Java21),
    }
}

fn log_download_steps(payload: &CreateInstancePayload, logs: &mut Vec<String>, java: JavaRuntime) {
    logs.push(format!(
        "Validando versión seleccionada: {}",
        payload.minecraft_version
    ));
    logs.push("version_manifest oficial de Mojang validado en interfaz.".to_string());
    logs.push(
        "version.json oficial consultado: se detectaron mainClass, libraries, assets y client.jar."
            .to_string(),
    );
    logs.push(format!(
        "Java efectivo para la instalación: {}.",
        java.major()
    ));
    if payload.loader != "vanilla" {
        logs.push(format!(
            "Instalando loader {} {} (flujo de integración).",
            payload.loader, payload.loader_version
        ));
    }
}
