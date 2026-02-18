use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Mutex, OnceLock},
    thread,
};

use serde::Serialize;
use serde_json::Value;
use sha1::{Digest, Sha1};
use zip::ZipArchive;

use crate::domain::auth::{
    microsoft::refresh_microsoft_access_token,
    xbox::{
        authenticate_with_xbox_live, authorize_xsts, has_minecraft_license,
        login_minecraft_with_xbox,
    },
};

use crate::{
    domain::{
        minecraft::{
            argument_resolver::{
                replace_launch_variables, resolve_launch_arguments, unresolved_variables_in_args,
                LaunchContext,
            },
            rule_engine::RuleContext,
        },
        models::instance::{InstanceMetadata, LaunchAuthSession},
        models::java::JavaRuntime,
    },
    services::java_installer::ensure_embedded_java,
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartInstanceResult {
    pub pid: u32,
    pub java_path: String,
    pub logs: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub running: bool,
    pub pid: Option<u32>,
    pub exit_code: Option<i32>,
    pub stderr_tail: Vec<String>,
}

#[derive(Debug, Clone)]
struct RuntimeState {
    pid: Option<u32>,
    running: bool,
    exit_code: Option<i32>,
    stderr_tail: Vec<String>,
}

#[derive(Debug, Clone)]
struct VerifiedLaunchAuth {
    profile_id: String,
    profile_name: String,
    minecraft_access_token: String,
    premium_verified: bool,
}

static RUNTIME_REGISTRY: OnceLock<Mutex<HashMap<String, RuntimeState>>> = OnceLock::new();

fn runtime_registry() -> &'static Mutex<HashMap<String, RuntimeState>> {
    RUNTIME_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

#[tauri::command]
pub fn get_runtime_status(instance_root: String) -> Result<RuntimeStatus, String> {
    let registry = runtime_registry()
        .lock()
        .map_err(|_| "No se pudo bloquear el registro de runtime.".to_string())?;

    if let Some(state) = registry.get(&instance_root) {
        return Ok(RuntimeStatus {
            running: state.running,
            pid: state.pid,
            exit_code: state.exit_code,
            stderr_tail: state.stderr_tail.clone(),
        });
    }

    Ok(RuntimeStatus {
        running: false,
        pid: None,
        exit_code: None,
        stderr_tail: Vec::new(),
    })
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
pub fn validate_and_prepare_launch(
    instance_root: String,
    auth_session: LaunchAuthSession,
) -> Result<LaunchValidationResult, String> {
    let instance_path = Path::new(&instance_root);
    if !instance_path.exists() {
        return Err("La instancia no existe en disco.".to_string());
    }

    let mut logs = vec!["🔹 1. Validaciones iniciales".to_string()];

    let metadata = get_instance_metadata(instance_root.clone())?;
    logs.push("✔ .instance.json leído correctamente".to_string());

    let verified_auth = validate_official_minecraft_auth(&auth_session, &mut logs)?;

    let embedded_java = ensure_instance_embedded_java(instance_path, &metadata, &mut logs)?;
    let java_path = PathBuf::from(&embedded_java);

    let java_output = Command::new(&java_path)
        .arg("-version")
        .output()
        .map_err(|err| format!("No se pudo validar versión de Java: {err}"))?;
    let java_version_text = String::from_utf8_lossy(&java_output.stderr).to_string();
    if !java_output.status.success() {
        return Err(format!("java -version falló: {}", java_version_text.trim()));
    }
    logs.push(format!(
        "✔ java -version detectado: {}",
        first_line(&java_version_text)
    ));

    let mc_root = instance_path.join("minecraft");
    let version_json = load_merged_version_json(&mc_root, &metadata.minecraft_version)?;

    let executable_version_id = version_json
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or(&metadata.minecraft_version)
        .to_string();
    let executable_jar = mc_root
        .join("versions")
        .join(&executable_version_id)
        .join(format!("{executable_version_id}.jar"));

    let client_jar = if executable_jar.exists() {
        executable_jar
    } else {
        let fallback = mc_root
            .join("versions")
            .join(&metadata.minecraft_version)
            .join(format!("{}.jar", &metadata.minecraft_version));
        if !fallback.exists() {
            return Err(format!(
                "Jar ejecutable no existe ni en versión efectiva ni fallback: {} | {}",
                mc_root
                    .join("versions")
                    .join(&executable_version_id)
                    .join(format!("{executable_version_id}.jar"))
                    .display(),
                fallback.display()
            ));
        }
        fallback
    };
    logs.push(format!(
        "✔ jar ejecutable presente: {}",
        client_jar.display()
    ));

    ensure_main_class_present_in_jar(&client_jar, "net.minecraft.client.main.Main")?;
    logs.push("✔ clase principal net.minecraft.client.main.Main verificada en jar".to_string());

    let rule_context = RuleContext::current();
    let mut resolved_libraries = resolve_libraries(&mc_root, &version_json, &rule_context);
    hydrate_missing_libraries(&resolved_libraries.missing_classpath_entries, &mut logs)?;
    ensure_assets_available(&mc_root, &version_json, &mut logs)?;
    resolved_libraries = resolve_libraries(&mc_root, &version_json, &rule_context);

    if !resolved_libraries.missing_classpath_entries.is_empty() {
        return Err(format!(
            "Hay librerías faltantes en disco ({}). Ejemplo: {}",
            resolved_libraries.missing_classpath_entries.len(),
            resolved_libraries
                .missing_classpath_entries
                .iter()
                .take(3)
                .map(|entry| entry.path.clone())
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }

    if !resolved_libraries.missing_native_entries.is_empty() {
        return Err(format!(
            "Faltan nativos requeridos para el OS actual ({}). Ejemplo: {}",
            resolved_libraries.missing_native_entries.len(),
            resolved_libraries
                .missing_native_entries
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }

    logs.push(format!(
        "✔ libraries evaluadas: {} (faltantes: 0)",
        resolved_libraries.classpath_entries.len()
    ));

    let mut jars_to_validate = resolved_libraries
        .classpath_entries
        .iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    jars_to_validate.push(client_jar.clone());
    jars_to_validate.extend(
        resolved_libraries
            .native_jars
            .iter()
            .map(|native| PathBuf::from(&native.path)),
    );
    validate_jars_as_zip(&jars_to_validate)?;
    logs.push(format!(
        "✔ jars validados como zip: {}",
        jars_to_validate.len()
    ));

    let natives_dir = mc_root.join("natives");
    extract_natives(&resolved_libraries.native_jars, &natives_dir)?;
    logs.push(format!(
        "✔ natives extraídos: {} archivos fuente en {}",
        resolved_libraries.native_jars.len(),
        natives_dir.display()
    ));

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
        format!(
            "⚠ assets index no encontrado todavía: {}",
            assets_index.display()
        )
    });

    logs.push("🔹 2. Preparación de ejecución".to_string());

    let sep = if cfg!(target_os = "windows") {
        ";"
    } else {
        ":"
    };
    let mut classpath_entries = resolved_libraries.classpath_entries.clone();
    classpath_entries.push(client_jar.display().to_string());
    let classpath = classpath_entries.join(sep);
    if classpath.trim().is_empty() {
        return Err("Classpath vacío luego del ensamblado final.".to_string());
    }
    logs.push(format!(
        "✔ classpath construido ({} entradas)",
        classpath_entries.len()
    ));

    let launch_context = LaunchContext {
        classpath: classpath.clone(),
        classpath_separator: sep.to_string(),
        library_directory: mc_root.join("libraries").display().to_string(),
        natives_dir: natives_dir.display().to_string(),
        launcher_name: "Interface-2".to_string(),
        launcher_version: env!("CARGO_PKG_VERSION").to_string(),
        auth_player_name: verified_auth.profile_name.clone(),
        auth_uuid: verified_auth.profile_id.clone(),
        auth_access_token: verified_auth.minecraft_access_token.clone(),
        user_type: "msa".to_string(),
        user_properties: "{}".to_string(),
        version_name: metadata.minecraft_version.clone(),
        game_directory: mc_root.display().to_string(),
        assets_root: mc_root.join("assets").display().to_string(),
        assets_index_name,
        version_type: "release".to_string(),
        resolution_width: "854".to_string(),
        resolution_height: "480".to_string(),
        clientid: String::new(),
        auth_xuid: String::new(),
        xuid: String::new(),
        quick_play_singleplayer: String::new(),
        quick_play_multiplayer: String::new(),
        quick_play_realms: String::new(),
        quick_play_path: String::new(),
    };

    let mut resolved =
        resolve_launch_arguments(&version_json, &launch_context, &RuleContext::current())?;
    let memory_args = vec![
        format!("-Xms{}M", metadata.ram_mb.max(512) / 2),
        format!("-Xmx{}M", metadata.ram_mb.max(512)),
    ];
    let mut jvm_args = memory_args;
    jvm_args.extend(
        metadata
            .java_args
            .iter()
            .map(|arg| replace_launch_variables(arg, &launch_context)),
    );
    jvm_args.append(&mut resolved.jvm);

    if !contains_classpath_switch(&jvm_args) {
        jvm_args.push("-cp".to_string());
        jvm_args.push(classpath.clone());
    }

    let unresolved_vars = unresolved_variables_in_args(jvm_args.iter().chain(resolved.game.iter()));
    if !unresolved_vars.is_empty() {
        logs.push(format!(
            "⚠ variables sin resolver detectadas: {:?}",
            unresolved_vars
        ));
        return Err(format!(
            "Hay variables sin resolver en argumentos JVM/Game: {}",
            unresolved_vars.join(", ")
        ));
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
    logs.push(
        "✔ Comando Java preparado con redirección de salida y consola en tiempo real".to_string(),
    );
    logs.push("🔹 5. Monitoreo".to_string());
    logs.push(
        "✔ Estrategia: detectar excepciones fatales, cierre inesperado y código de salida"
            .to_string(),
    );
    logs.push("🔹 6. Finalización".to_string());
    logs.push("✔ Manejo de cierre normal/error y persistencia de log completo".to_string());

    if !verified_auth.premium_verified {
        return Err("La cuenta no posee licencia oficial de Minecraft.".to_string());
    }

    Ok(LaunchValidationResult {
        java_path: embedded_java,
        java_version: first_line(&java_version_text),
        classpath,
        jvm_args,
        game_args: resolved.game,
        main_class: resolved.main_class,
        logs,
    })
}

#[tauri::command]
pub fn start_instance(
    instance_root: String,
    auth_session: LaunchAuthSession,
) -> Result<StartInstanceResult, String> {
    {
        let mut registry = runtime_registry()
            .lock()
            .map_err(|_| "No se pudo bloquear el registro de runtime.".to_string())?;
        if let Some(state) = registry.get(&instance_root) {
            if state.running {
                return Err(
                    "La instancia ya está ejecutándose; no se permite doble ejecución.".to_string(),
                );
            }
        }
        registry.insert(
            instance_root.clone(),
            RuntimeState {
                pid: None,
                running: true,
                exit_code: None,
                stderr_tail: Vec::new(),
            },
        );
    }

    let prepared = match validate_and_prepare_launch(instance_root.clone(), auth_session) {
        Ok(value) => value,
        Err(err) => {
            if let Ok(mut registry) = runtime_registry().lock() {
                registry.remove(&instance_root);
            }
            return Err(err);
        }
    };

    let mut command = Command::new(&prepared.java_path);
    command
        .args(&prepared.jvm_args)
        .arg(&prepared.main_class)
        .args(&prepared.game_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match command
        .spawn()
        .map_err(|err| format!("No se pudo iniciar java para la instancia: {err}"))
    {
        Ok(child) => child,
        Err(err) => {
            if let Ok(mut registry) = runtime_registry().lock() {
                registry.remove(&instance_root);
            }
            return Err(err);
        }
    };

    let pid = child.id();
    if let Ok(mut registry) = runtime_registry().lock() {
        if let Some(state) = registry.get_mut(&instance_root) {
            state.pid = Some(pid);
        }
    }

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let instance_root_for_thread = instance_root.clone();

    thread::spawn(move || {
        let mut stderr_tail = Vec::<String>::new();

        if let Some(stdout_pipe) = stdout {
            let reader = BufReader::new(stdout_pipe);
            for line in reader.lines().map_while(Result::ok) {
                if line.trim().is_empty() {
                    continue;
                }
                log::info!("[MC-STDOUT][{}] {}", instance_root_for_thread, line);
            }
        }

        if let Some(stderr_pipe) = stderr {
            let reader = BufReader::new(stderr_pipe);
            for line in reader.lines().map_while(Result::ok) {
                if line.trim().is_empty() {
                    continue;
                }
                log::warn!("[MC-STDERR][{}] {}", instance_root_for_thread, line);
                stderr_tail.push(line);
                if stderr_tail.len() > 100 {
                    let drop_count = stderr_tail.len() - 100;
                    stderr_tail.drain(0..drop_count);
                }
            }
        }

        let exit_code = child.wait().ok().and_then(|status| status.code());

        if let Ok(mut registry) = runtime_registry().lock() {
            registry.insert(
                instance_root_for_thread,
                RuntimeState {
                    pid: Some(pid),
                    running: false,
                    exit_code,
                    stderr_tail,
                },
            );
        }
    });

    Ok(StartInstanceResult {
        pid,
        java_path: prepared.java_path,
        logs: vec![
            "Comando de lanzamiento ejecutado con argumentos validados.".to_string(),
            "Salida estándar y de error conectadas para monitoreo; exit_code persistido al finalizar.".to_string(),
        ],
    })
}

fn first_line(text: &str) -> String {
    text.lines()
        .next()
        .unwrap_or("desconocido")
        .trim()
        .to_string()
}

fn ensure_instance_embedded_java(
    instance_path: &Path,
    metadata: &InstanceMetadata,
    logs: &mut Vec<String>,
) -> Result<String, String> {
    let launcher_root = instance_path
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| {
            format!(
                "No se pudo resolver launcher_root desde instancia {}",
                instance_path.display()
            )
        })?;

    let runtime = parse_runtime_from_metadata(metadata).ok_or_else(|| {
        format!(
            "No se pudo determinar java_runtime para la instancia '{}'. Valor recibido: '{}'",
            metadata.name, metadata.java_runtime
        )
    })?;

    let java_exec = ensure_embedded_java(launcher_root, runtime, logs)?;
    logs.push(format!(
        "✔ runtime embebido garantizado para Java {}: {}",
        runtime.major(),
        java_exec.display()
    ));

    if Path::new(&metadata.java_path) != java_exec {
        persist_instance_java_path(instance_path, metadata, &java_exec, logs)?;
    }

    Ok(java_exec.display().to_string())
}

fn validate_official_minecraft_auth(
    auth_session: &LaunchAuthSession,
    logs: &mut Vec<String>,
) -> Result<VerifiedLaunchAuth, String> {
    if !auth_session.premium_verified {
        return Err("La cuenta no posee licencia oficial de Minecraft.".to_string());
    }

    if auth_session.minecraft_access_token.trim().is_empty() {
        return Err(
            "No hay access token de Minecraft válido; no se permite iniciar en modo Demo."
                .to_string(),
        );
    }

    if auth_session.profile_name.trim().is_empty() || auth_session.profile_id.trim().is_empty() {
        return Err(
            "No hay perfil oficial de Minecraft (name/uuid); no se permite iniciar en modo Demo."
                .to_string(),
        );
    }

    let client = reqwest::blocking::Client::new();
    let mut active_minecraft_token = auth_session.minecraft_access_token.clone();

    let mut profile_response = client
        .get("https://api.minecraftservices.com/minecraft/profile")
        .header(
            "Authorization",
            format!("Bearer {}", active_minecraft_token),
        )
        .header("Accept", "application/json")
        .send()
        .map_err(|err| format!("No se pudo consultar perfil de Minecraft: {err}"))?;

    if profile_response.status().as_u16() == 401 {
        logs.push(
            "⚠ access_token expirado; intentando refresh oficial Microsoft/Xbox/XSTS..."
                .to_string(),
        );
        let refresh_token = auth_session
            .microsoft_refresh_token
            .clone()
            .ok_or_else(|| {
                "El access token expiró y no hay refresh token; ejecución bloqueada.".to_string()
            })?;

        let runtime = tokio::runtime::Runtime::new()
            .map_err(|err| format!("No se pudo crear runtime para refresh de token: {err}"))?;

        let refreshed_mc = runtime.block_on(async {
            let ms =
                refresh_microsoft_access_token(&reqwest::Client::new(), &refresh_token).await?;
            let xbox =
                authenticate_with_xbox_live(&reqwest::Client::new(), &ms.access_token).await?;
            let xsts = authorize_xsts(&reqwest::Client::new(), &xbox.token).await?;
            let mc =
                login_minecraft_with_xbox(&reqwest::Client::new(), &xsts.uhs, &xsts.token).await?;
            Ok::<String, String>(mc.access_token)
        })?;

        active_minecraft_token = refreshed_mc;
        profile_response = client
            .get("https://api.minecraftservices.com/minecraft/profile")
            .header(
                "Authorization",
                format!("Bearer {}", active_minecraft_token),
            )
            .header("Accept", "application/json")
            .send()
            .map_err(|err| {
                format!("No se pudo consultar perfil de Minecraft tras refresh: {err}")
            })?;
    }

    let profile_status = profile_response.status();
    if !profile_status.is_success() {
        let body = profile_response.text().unwrap_or_default();
        return Err(format!(
            "La API de perfil de Minecraft devolvió error HTTP: {profile_status}. Body completo: {body}"
        ));
    }

    let profile = profile_response
        .json::<serde_json::Value>()
        .map_err(|err| format!("No se pudo leer perfil de Minecraft: {err}"))?;

    let profile_id = profile
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let profile_name = profile
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();

    if profile_id.is_empty() || profile_name.is_empty() {
        return Err(
            "El perfil de Minecraft no devolvió id/name válidos; ejecución bloqueada.".to_string(),
        );
    }

    if profile_id != auth_session.profile_id || profile_name != auth_session.profile_name {
        return Err("El perfil de Minecraft no coincide con la sesión actual; token inválido o vencido. Se bloquea para evitar modo Demo.".to_string());
    }

    let runtime = tokio::runtime::Runtime::new()
        .map_err(|err| format!("No se pudo crear runtime para validar entitlements: {err}"))?;
    let has_license = runtime.block_on(async {
        has_minecraft_license(&reqwest::Client::new(), &active_minecraft_token).await
    })?;

    if !has_license {
        return Err("La cuenta no posee licencia oficial de Minecraft.".to_string());
    }

    logs.push("✔ Licencia oficial verificada en entitlements/mcstore (sin Demo).".to_string());
    logs.push(format!(
        "✔ Perfil oficial verificado: {} ({})",
        profile_name, profile_id
    ));

    Ok(VerifiedLaunchAuth {
        profile_id,
        profile_name,
        minecraft_access_token: active_minecraft_token,
        premium_verified: true,
    })
}

fn contains_classpath_switch(jvm_args: &[String]) -> bool {
    if jvm_args
        .iter()
        .any(|arg| arg.starts_with("-cp=") || arg.starts_with("-classpath="))
    {
        return true;
    }

    jvm_args
        .windows(2)
        .any(|window| matches!(window, [flag, _value] if flag == "-cp" || flag == "-classpath"))
}

#[derive(Debug, Clone)]
struct MissingLibraryEntry {
    path: String,
    url: String,
    sha1: String,
}

#[derive(Debug, Clone)]
struct NativeJarEntry {
    path: String,
}

#[derive(Debug, Clone)]
struct ResolvedLibraries {
    classpath_entries: Vec<String>,
    missing_classpath_entries: Vec<MissingLibraryEntry>,
    native_jars: Vec<NativeJarEntry>,
    missing_native_entries: Vec<String>,
}

fn load_merged_version_json(mc_root: &Path, version_id: &str) -> Result<Value, String> {
    fn load_version_json(mc_root: &Path, version_id: &str) -> Result<Value, String> {
        let path = mc_root
            .join("versions")
            .join(version_id)
            .join(format!("{version_id}.json"));
        let raw = fs::read_to_string(&path)
            .map_err(|err| format!("No se pudo leer version json {}: {err}", path.display()))?;
        serde_json::from_str(&raw)
            .map_err(|err| format!("No se pudo parsear version json {}: {err}", path.display()))
    }

    fn merge_values(base: Value, child: Value) -> Value {
        match (base, child) {
            (Value::Object(mut b), Value::Object(c)) => {
                for (key, child_value) in c {
                    let merged = match b.remove(&key) {
                        Some(base_value)
                            if key == "arguments" || key == "downloads" || key == "assetIndex" =>
                        {
                            merge_values(base_value, child_value)
                        }
                        _ => child_value,
                    };
                    b.insert(key, merged);
                }
                Value::Object(b)
            }
            (_, child) => child,
        }
    }

    let child = load_version_json(mc_root, version_id)?;
    if let Some(parent_id) = child.get("inheritsFrom").and_then(Value::as_str) {
        let parent = load_merged_version_json(mc_root, parent_id)?;
        Ok(merge_values(parent, child))
    } else {
        Ok(child)
    }
}

fn ensure_main_class_present_in_jar(jar_path: &Path, main_class: &str) -> Result<(), String> {
    let file = fs::File::open(jar_path)
        .map_err(|err| format!("No se pudo abrir jar {}: {err}", jar_path.display()))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|err| format!("Jar inválido {}: {err}", jar_path.display()))?;
    let class_entry = format!("{}.class", main_class.replace('.', "/"));
    archive.by_name(&class_entry).map(|_| ()).map_err(|_| {
        format!(
            "La clase principal {main_class} no existe en {}",
            jar_path.display()
        )
    })
}

fn build_maven_library_path(mc_root: &Path, library: &Value) -> Option<String> {
    let name = library.get("name")?.as_str()?;
    let mut parts = name.split(':');
    let group = parts.next()?;
    let artifact = parts.next()?;
    let version = parts.next()?;
    let classifier_and_ext = parts.next();

    let group_path = group.replace('.', "/");
    let (classifier, extension) = if let Some(rest) = classifier_and_ext {
        if let Some((classifier, ext)) = rest.split_once('@') {
            (Some(classifier.to_string()), ext.to_string())
        } else {
            (Some(rest.to_string()), "jar".to_string())
        }
    } else {
        (None, "jar".to_string())
    };

    let file_name = if let Some(classifier) = classifier {
        format!("{artifact}-{version}-{classifier}.{extension}")
    } else {
        format!("{artifact}-{version}.{extension}")
    };

    Some(
        mc_root
            .join("libraries")
            .join(group_path)
            .join(artifact)
            .join(version)
            .join(file_name)
            .display()
            .to_string(),
    )
}

fn resolve_libraries(
    mc_root: &Path,
    version_json: &Value,
    rule_context: &RuleContext,
) -> ResolvedLibraries {
    let mut classpath_entries = Vec::new();
    let mut missing_classpath_entries = Vec::new();
    let mut native_jars = Vec::new();
    let mut missing_native_entries = Vec::new();

    let os_key = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "osx"
    };

    for lib in version_json
        .get("libraries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let rules = lib
            .get("rules")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if !crate::domain::minecraft::rule_engine::evaluate_rules(&rules, rule_context) {
            continue;
        }

        let artifact_path = lib
            .get("downloads")
            .and_then(|v| v.get("artifact"))
            .and_then(|v| v.get("path"))
            .and_then(Value::as_str)
            .map(|p| mc_root.join("libraries").join(p).display().to_string())
            .or_else(|| build_maven_library_path(mc_root, &lib));

        if let Some(path) = artifact_path {
            if Path::new(&path).exists() {
                classpath_entries.push(path);
            } else {
                let artifact = lib.get("downloads").and_then(|v| v.get("artifact"));
                let url = artifact
                    .and_then(|v| v.get("url"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let sha1 = artifact
                    .and_then(|v| v.get("sha1"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();

                if !url.is_empty() && !sha1.is_empty() {
                    missing_classpath_entries.push(MissingLibraryEntry { path, url, sha1 });
                } else {
                    missing_native_entries.push(format!(
                        "metadata incompleta para descargar librería faltante: {}",
                        lib.get("name").and_then(Value::as_str).unwrap_or("unknown")
                    ));
                }
            }
        }

        let native_classifier = lib
            .get("natives")
            .and_then(|v| v.get(os_key))
            .and_then(Value::as_str);

        if let Some(classifier) = native_classifier {
            let native_key = classifier.replace("${arch}", std::env::consts::ARCH);
            let native_path = lib
                .get("downloads")
                .and_then(|v| v.get("classifiers"))
                .and_then(|v| v.get(&native_key))
                .and_then(|v| v.get("path"))
                .and_then(Value::as_str)
                .map(|p| mc_root.join("libraries").join(p).display().to_string());

            match native_path {
                Some(path) if Path::new(&path).exists() => {
                    native_jars.push(NativeJarEntry { path })
                }
                Some(path) => missing_native_entries.push(path),
                None => missing_native_entries.push(format!(
                    "native no encontrado para {} ({native_key})",
                    lib.get("name").and_then(Value::as_str).unwrap_or("unknown")
                )),
            }
        }
    }

    ResolvedLibraries {
        classpath_entries,
        missing_classpath_entries,
        native_jars,
        missing_native_entries,
    }
}

fn hydrate_missing_libraries(
    missing_entries: &[MissingLibraryEntry],
    logs: &mut Vec<String>,
) -> Result<(), String> {
    if missing_entries.is_empty() {
        return Ok(());
    }

    let client = reqwest::blocking::Client::new();

    for entry in missing_entries {
        let path = Path::new(&entry.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "No se pudo crear directorio para librería faltante {}: {err}",
                    parent.display()
                )
            })?;
        }

        let bytes = client
            .get(&entry.url)
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(|err| {
                format!(
                    "No se pudo descargar librería faltante {}: {err}",
                    entry.url
                )
            })?
            .bytes()
            .map_err(|err| format!("No se pudo leer librería faltante {}: {err}", entry.url))?;

        let actual_sha1 = sha1_hex(bytes.as_ref());
        if !actual_sha1.eq_ignore_ascii_case(&entry.sha1) {
            return Err(format!(
                "SHA1 inválido en librería {}. Esperado {}, obtenido {}",
                entry.path, entry.sha1, actual_sha1
            ));
        }

        fs::write(path, &bytes).map_err(|err| {
            format!(
                "No se pudo guardar librería faltante {}: {err}",
                path.display()
            )
        })?;

        logs.push(format!(
            "✔ librería faltante descargada en runtime: {}",
            entry.path
        ));
    }
    Ok(())
}

fn ensure_assets_available(
    mc_root: &Path,
    version_json: &Value,
    logs: &mut Vec<String>,
) -> Result<(), String> {
    let asset_index = version_json
        .get("assetIndex")
        .ok_or_else(|| "version.json no incluye assetIndex".to_string())?;

    let index_url = asset_index
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| "assetIndex.url no está presente".to_string())?;
    let index_id = asset_index
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "assetIndex.id no está presente".to_string())?;

    let indexes_dir = mc_root.join("assets").join("indexes");
    fs::create_dir_all(&indexes_dir).map_err(|err| {
        format!(
            "No se pudo crear carpeta de asset indexes {}: {err}",
            indexes_dir.display()
        )
    })?;

    let index_path = indexes_dir.join(format!("{index_id}.json"));
    if !index_path.exists() {
        let index_bytes = reqwest::blocking::get(index_url)
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(|err| format!("No se pudo descargar asset index oficial: {err}"))?
            .bytes()
            .map_err(|err| format!("No se pudo leer asset index oficial: {err}"))?;
        fs::write(&index_path, &index_bytes).map_err(|err| {
            format!(
                "No se pudo guardar asset index en {}: {err}",
                index_path.display()
            )
        })?;
    }

    let raw_index = fs::read_to_string(&index_path).map_err(|err| {
        format!(
            "No se pudo leer asset index {}: {err}",
            index_path.display()
        )
    })?;

    let parsed: Value = serde_json::from_str(&raw_index)
        .map_err(|err| format!("asset index inválido {}: {err}", index_path.display()))?;

    let objects = parsed
        .get("objects")
        .and_then(Value::as_object)
        .ok_or_else(|| format!("asset index sin objects: {}", index_path.display()))?;

    let mut missing = Vec::new();
    for object in objects.values() {
        let Some(hash) = object.get("hash").and_then(Value::as_str) else {
            continue;
        };
        if hash.len() < 2 {
            continue;
        }
        let prefix = &hash[..2];
        let object_path = mc_root
            .join("assets")
            .join("objects")
            .join(prefix)
            .join(hash);
        if !object_path.exists() {
            missing.push((hash.to_string(), object_path));
        }
    }

    if missing.is_empty() {
        logs.push("✔ assets listos en disco (sin descargas diferidas).".to_string());
        return Ok(());
    }

    logs.push(format!(
        "ℹ assets faltantes detectados para esta ejecución: {}",
        missing.len()
    ));

    let client = reqwest::blocking::Client::new();
    for (idx, (hash, object_path)) in missing.iter().enumerate() {
        if let Some(parent) = object_path.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "No se pudo crear carpeta de asset {}: {err}",
                    parent.display()
                )
            })?;
        }

        let prefix = &hash[..2];
        let url = format!("https://resources.download.minecraft.net/{prefix}/{hash}");
        let bytes = client
            .get(&url)
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(|err| format!("No se pudo descargar asset {hash}: {err}"))?
            .bytes()
            .map_err(|err| format!("No se pudo leer asset {hash}: {err}"))?;

        let actual_sha1 = sha1_hex(bytes.as_ref());
        if !actual_sha1.eq_ignore_ascii_case(hash) {
            return Err(format!(
                "SHA1 inválido en asset {}. Esperado {}, obtenido {}",
                object_path.display(),
                hash,
                actual_sha1
            ));
        }

        fs::write(object_path, &bytes)
            .map_err(|err| format!("No se pudo guardar asset {}: {err}", object_path.display()))?;

        if (idx + 1) % 250 == 0 || idx + 1 == missing.len() {
            logs.push(format!(
                "✔ assets diferidos descargados: {}/{}",
                idx + 1,
                missing.len()
            ));
        }
    }

    Ok(())
}

fn sha1_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn validate_jars_as_zip(jars: &[PathBuf]) -> Result<(), String> {
    for jar in jars {
        let file = fs::File::open(jar)
            .map_err(|err| format!("No se pudo abrir jar {}: {err}", jar.display()))?;
        ZipArchive::new(file)
            .map_err(|err| format!("Jar inválido/corrupto {}: {err}", jar.display()))?;
    }
    Ok(())
}

fn extract_natives(native_jars: &[NativeJarEntry], natives_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(natives_dir).map_err(|err| {
        format!(
            "No se pudo crear carpeta natives {}: {err}",
            natives_dir.display()
        )
    })?;

    for native in native_jars {
        let file = fs::File::open(&native.path)
            .map_err(|err| format!("No se pudo abrir native jar {}: {err}", native.path))?;
        let mut archive = ZipArchive::new(file)
            .map_err(|err| format!("Native jar inválido {}: {err}", native.path))?;
        for i in 0..archive.len() {
            let mut entry = archive
                .by_index(i)
                .map_err(|err| format!("No se pudo leer entrada zip en {}: {err}", native.path))?;
            let name = entry.name().to_string();
            if entry.is_dir() || name.starts_with("META-INF/") {
                continue;
            }
            let out_path = natives_dir.join(&name);
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent).map_err(|err| {
                    format!(
                        "No se pudo crear directorio de natives {}: {err}",
                        parent.display()
                    )
                })?;
            }
            let mut out = fs::File::create(&out_path)
                .map_err(|err| format!("No se pudo crear native {}: {err}", out_path.display()))?;
            std::io::copy(&mut entry, &mut out).map_err(|err| {
                format!("No se pudo extraer native {}: {err}", out_path.display())
            })?;
        }
    }
    Ok(())
}

fn parse_runtime_major(input: &str) -> Option<JavaRuntime> {
    let digits = input
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>();
    let major = digits.parse::<u32>().ok()?;
    match major {
        0..=11 => Some(JavaRuntime::Java8),
        12..=20 => Some(JavaRuntime::Java17),
        _ => Some(JavaRuntime::Java21),
    }
}

fn parse_runtime_from_metadata(metadata: &InstanceMetadata) -> Option<JavaRuntime> {
    let normalized = metadata.java_runtime.to_lowercase();
    if normalized.contains("21") {
        return Some(JavaRuntime::Java21);
    }
    if normalized.contains("17") {
        return Some(JavaRuntime::Java17);
    }
    if normalized.contains('8') {
        return Some(JavaRuntime::Java8);
    }

    parse_runtime_major(&metadata.java_version).or_else(|| parse_runtime_major(&metadata.java_path))
}

fn persist_instance_java_path(
    instance_path: &Path,
    metadata: &InstanceMetadata,
    java_exec: &Path,
    logs: &mut Vec<String>,
) -> Result<(), String> {
    let mut updated = metadata.clone();
    updated.java_path = java_exec.display().to_string();
    updated.java_runtime = format!(
        "java{}",
        parse_runtime_from_metadata(&updated)
            .map(|r| r.major())
            .unwrap_or(17)
    );
    updated.java_version = format!(
        "{}.0.x",
        parse_runtime_from_metadata(&updated)
            .map(|r| r.major())
            .unwrap_or(17)
    );

    let metadata_path = instance_path.join(".instance.json");
    fs::write(
        &metadata_path,
        serde_json::to_string_pretty(&updated)
            .map_err(|err| format!("No se pudo serializar metadata actualizada: {err}"))?,
    )
    .map_err(|err| {
        format!(
            "No se pudo persistir metadata actualizada en {}: {err}",
            metadata_path.display()
        )
    })?;

    logs.push(format!(
        "✔ .instance.json actualizado con java_path embebido: {}",
        java_exec.display()
    ));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        build_maven_library_path, contains_classpath_switch, parse_runtime_from_metadata,
        parse_runtime_major,
    };
    use crate::domain::models::{instance::InstanceMetadata, java::JavaRuntime};
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn maven_fallback_supports_classifier_and_extension() {
        let lib = json!({"name": "org.lwjgl:lwjgl:3.3.1:natives-linux@zip"});

        let path = build_maven_library_path(Path::new("/tmp/mc"), &lib).unwrap();

        assert_eq!(
            path,
            "/tmp/mc/libraries/org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-linux.zip"
        );
    }

    #[test]
    fn classpath_switch_detects_equals_style_flags() {
        let jvm_args = vec!["-Xmx2G".to_string(), "-classpath=/tmp/cp".to_string()];

        assert!(contains_classpath_switch(&jvm_args));
    }

    #[test]
    fn parse_runtime_major_maps_expected_ranges() {
        assert_eq!(parse_runtime_major("8"), Some(JavaRuntime::Java8));
        assert_eq!(parse_runtime_major("17.0.10"), Some(JavaRuntime::Java17));
        assert_eq!(parse_runtime_major("21"), Some(JavaRuntime::Java21));
    }

    #[test]
    fn parse_runtime_from_metadata_uses_fallback_fields() {
        let metadata = InstanceMetadata {
            name: "Demo".to_string(),
            group: "Default".to_string(),
            minecraft_version: "1.20.4".to_string(),
            loader: "vanilla".to_string(),
            loader_version: "".to_string(),
            ram_mb: 2048,
            java_args: vec![],
            java_path: "C:/runtime/java17/bin/java.exe".to_string(),
            java_runtime: "desconocido".to_string(),
            java_version: "17.0.x".to_string(),
            last_used: None,
            internal_uuid: "id".to_string(),
        };

        assert_eq!(
            parse_runtime_from_metadata(&metadata),
            Some(JavaRuntime::Java17)
        );
    }
}
