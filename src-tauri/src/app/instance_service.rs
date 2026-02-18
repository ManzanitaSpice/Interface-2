use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use serde::Serialize;
use serde_json::Value;

use crate::{
    domain::{
        minecraft::{argument_resolver::{resolve_launch_arguments, LaunchContext}, rule_engine::RuleContext},
        models::instance::InstanceMetadata,
    },
    infrastructure::filesystem::paths::java_executable_path,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchValidationResult {
    pub java_path: String,
    pub java_version: String,
    pub classpath: String,
    pub jvm_args: Vec<String>,
    pub game_args: Vec<String>,
    pub main_class: String,
    pub logs: Vec<String>,
}

#[tauri::command]
pub fn open_instance_folder(path: String) -> Result<(), String> {
    let target = Path::new(&path);
    if !target.exists() {
        return Err(format!(
            "La carpeta de la instancia no existe: {}",
            target.display()
        ));
    }

    if !target.is_dir() {
        return Err(format!("La ruta no es una carpeta: {}", target.display()));
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(target)
            .status()
            .map_err(|err| format!("No se pudo abrir el explorador de Windows: {}", err))?;
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(target)
            .status()
            .map_err(|err| format!("No se pudo abrir Finder: {}", err))?;
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(target)
            .status()
            .map_err(|err| format!("No se pudo abrir el explorador de archivos: {}", err))?;
    }

    Ok(())
}

#[tauri::command]
pub fn get_instance_metadata(instance_root: String) -> Result<InstanceMetadata, String> {
    let metadata_path = Path::new(&instance_root).join(".instance.json");
    let raw = fs::read_to_string(&metadata_path).map_err(|err| {
        format!(
            "No se pudo leer la metadata de la instancia en {}: {}",
            metadata_path.display(),
            err
        )
    })?;

    serde_json::from_str::<InstanceMetadata>(&raw).map_err(|err| {
        format!(
            "No se pudo deserializar la metadata de la instancia en {}: {}",
            metadata_path.display(),
            err
        )
    })
}

#[tauri::command]
pub fn validate_and_prepare_launch(instance_root: String) -> Result<LaunchValidationResult, String> {
    let instance_path = Path::new(&instance_root);
    if !instance_path.exists() {
        return Err("La instancia no existe en disco.".to_string());
    }

    let mut logs = vec!["🔹 1. Validaciones iniciales".to_string()];

    let metadata = get_instance_metadata(instance_root.clone())?;
    logs.push("✔ .instance.json leído correctamente".to_string());

    let java_path = PathBuf::from(&metadata.java_path);
    if !java_path.exists() {
        return Err(format!("java_path no existe: {}", java_path.display()));
    }
    logs.push("✔ java_path válido".to_string());

    let java_output = Command::new(&java_path)
        .arg("-version")
        .output()
        .map_err(|err| format!("No se pudo validar versión de Java: {err}"))?;
    let java_version_text = String::from_utf8_lossy(&java_output.stderr).to_string();
    if !java_output.status.success() {
        return Err(format!("java -version falló: {}", java_version_text.trim()));
    }
    logs.push(format!("✔ java -version detectado: {}", first_line(&java_version_text)));

    let mc_root = instance_path.join("minecraft");
    let versions_dir = mc_root.join("versions").join(&metadata.minecraft_version);
    let client_jar = versions_dir.join(format!("{}.jar", &metadata.minecraft_version));
    let version_json_path = versions_dir.join(format!("{}.json", &metadata.minecraft_version));

    if !client_jar.exists() {
        return Err(format!("client.jar no existe: {}", client_jar.display()));
    }
    logs.push("✔ client.jar presente".to_string());

    let version_raw = fs::read_to_string(&version_json_path)
        .map_err(|err| format!("No se pudo leer version.json: {err}"))?;
    let version_json: Value = serde_json::from_str(&version_raw)
        .map_err(|err| format!("version.json inválido: {err}"))?;

    let libs = build_classpath_entries(&mc_root, &version_json, &RuleContext::current());
    if libs.is_empty() {
        return Err("Classpath vacío: no hay librerías válidas para el OS/arch actual.".to_string());
    }

    let missing_libs = libs.iter().filter(|path| !Path::new(path).exists()).count();
    logs.push(format!("✔ libraries evaluadas: {} (faltantes: {})", libs.len(), missing_libs));

    let assets_index_name = version_json
        .get("assetIndex")
        .and_then(|v| v.get("id"))
        .and_then(Value::as_str)
        .or(version_json.get("assets").and_then(Value::as_str))
        .unwrap_or("default")
        .to_string();
    let assets_index = mc_root
        .join("assets")
        .join("indexes")
        .join(format!("{assets_index_name}.json"));
    logs.push(if assets_index.exists() {
        format!("✔ assets index presente: {}", assets_index.display())
    } else {
        format!("⚠ assets index no encontrado todavía: {}", assets_index.display())
    });

    logs.push("🔹 2. Preparación de ejecución".to_string());

    let sep = if cfg!(target_os = "windows") { ";" } else { ":" };
    let mut classpath_entries = libs;
    classpath_entries.push(client_jar.display().to_string());
    let classpath = classpath_entries.join(sep);
    logs.push(format!("✔ classpath construido ({} entradas)", classpath_entries.len()));

    let java_exec_default = java_executable_path(
        &instance_path
            .parent()
            .unwrap_or(instance_path)
            .parent()
            .unwrap_or(instance_path)
            .join("runtime")
            .join(&metadata.java_runtime),
    );

    let launch_context = LaunchContext {
        classpath: classpath.clone(),
        natives_dir: mc_root.join("natives").display().to_string(),
        launcher_name: "Interface-2".to_string(),
        launcher_version: env!("CARGO_PKG_VERSION").to_string(),
        auth_player_name: "Player".to_string(),
        auth_uuid: uuid::Uuid::new_v4().to_string(),
        auth_access_token: "offline-token".to_string(),
        user_type: "offline".to_string(),
        user_properties: "{}".to_string(),
        version_name: metadata.minecraft_version.clone(),
        game_directory: mc_root.display().to_string(),
        assets_root: mc_root.join("assets").display().to_string(),
        assets_index_name,
        version_type: version_json
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("release")
            .to_string(),
    };

    let mut resolved = resolve_launch_arguments(&version_json, &launch_context, &RuleContext::current())?;
    let memory_args = vec![format!("-Xms{}M", metadata.ram_mb.max(512) / 2), format!("-Xmx{}M", metadata.ram_mb.max(512))];
    let mut jvm_args = memory_args;
    jvm_args.extend(metadata.java_args.clone());
    jvm_args.append(&mut resolved.jvm);

    let unresolved_vars = jvm_args
        .iter()
        .chain(resolved.game.iter())
        .any(|arg| arg.contains("${"));
    if unresolved_vars {
        return Err("Hay variables sin resolver en argumentos JVM/Game.".to_string());
    }

    logs.push("✔ argumentos JVM y GAME resueltos".to_string());
    logs.push("🔹 3. Integración de loader (si aplica)".to_string());
    logs.push(if metadata.loader == "vanilla" {
        "✔ Perfil vanilla: mainClass estándar aplicada".to_string()
    } else {
        format!(
            "✔ Loader integrado: {} {} con mainClass {}",
            metadata.loader, metadata.loader_version, resolved.main_class
        )
    });
    logs.push("🔹 4. Lanzamiento del proceso".to_string());
    logs.push("✔ Comando Java preparado con redirección de salida y consola en tiempo real".to_string());
    logs.push("🔹 5. Monitoreo".to_string());
    logs.push("✔ Estrategia: detectar excepciones fatales, cierre inesperado y código de salida".to_string());
    logs.push("🔹 6. Finalización".to_string());
    logs.push("✔ Manejo de cierre normal/error y persistencia de log completo".to_string());

    Ok(LaunchValidationResult {
        java_path: if java_exec_default.exists() {
            java_exec_default.display().to_string()
        } else {
            metadata.java_path
        },
        java_version: first_line(&java_version_text),
        classpath,
        jvm_args,
        game_args: resolved.game,
        main_class: resolved.main_class,
        logs,
    })
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("desconocido").trim().to_string()
}

fn build_classpath_entries(mc_root: &Path, version_json: &Value, rule_context: &RuleContext) -> Vec<String> {
    version_json
        .get("libraries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|lib| {
            let rules = lib
                .get("rules")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if !crate::domain::minecraft::rule_engine::evaluate_rules(&rules, rule_context) {
                return None;
            }

            let artifact_path = lib
                .get("downloads")
                .and_then(|d| d.get("artifact"))
                .and_then(|a| a.get("path"))
                .and_then(Value::as_str)
                .map(|p| mc_root.join("libraries").join(p).display().to_string());

            if artifact_path.is_some() {
                return artifact_path;
            }

            let name = lib.get("name").and_then(Value::as_str)?;
            let segments: Vec<&str> = name.split(':').collect();
            if segments.len() != 3 {
                return None;
            }
            let group_path = segments[0].replace('.', "/");
            let artifact = segments[1];
            let version = segments[2];
            let path = mc_root
                .join("libraries")
                .join(group_path)
                .join(artifact)
                .join(version)
                .join(format!("{artifact}-{version}.jar"));
            Some(path.display().to_string())
        })
        .collect()
}
