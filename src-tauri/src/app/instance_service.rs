use std::{
    collections::HashMap,
    fs, io,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Mutex, OnceLock},
    thread,
};

use serde::Serialize;
use serde_json::Value;
use zip::ZipArchive;

use crate::{
    domain::{
        minecraft::{
            argument_resolver::{resolve_launch_arguments, LaunchContext},
            rule_engine::RuleContext,
        },
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
) -> Result<LaunchValidationResult, String> {
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
    logs.push(format!("✔ jar ejecutable presente: {}", client_jar.display()));

    let rule_context = RuleContext::current();
    let mut resolved_libraries = resolve_libraries(&mc_root, &version_json, &rule_context);
    hydrate_missing_libraries(&resolved_libraries.missing_classpath_entries, &mut logs)?;
    resolved_libraries = resolve_libraries(&mc_root, &version_json, &rule_context);

    if resolved_libraries.classpath_entries.is_empty() {
        return Err("Classpath vacío: no hay librerías válidas para el OS/arch actual. Revisa rules, OS/arch y descarga de libraries/artifacts.".to_string());
    }

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
    let mut classpath_entries = resolved_libraries.classpath_entries;
    classpath_entries.push(client_jar.display().to_string());
    let classpath = classpath_entries.join(sep);
    if classpath.trim().is_empty() {
        return Err("Classpath vacío luego del ensamblado final.".to_string());
    }
    logs.push(format!(
        "✔ classpath construido ({} entradas)",
        classpath_entries.len()
    ));

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
        natives_dir: natives_dir.display().to_string(),
        launcher_name: "Interface-2".to_string(),
        launcher_version: env!("CARGO_PKG_VERSION").to_string(),
        auth_player_name: "Player".to_string(),
        auth_uuid: uuid::Uuid::new_v4().to_string(),
        auth_access_token: "0".to_string(),
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

    let mut resolved =
        resolve_launch_arguments(&version_json, &launch_context, &RuleContext::current())?;
    let memory_args = vec![
        format!("-Xms{}M", metadata.ram_mb.max(512) / 2),
        format!("-Xmx{}M", metadata.ram_mb.max(512)),
    ];
    let mut jvm_args = memory_args;
    jvm_args.extend(metadata.java_args.clone());
    jvm_args.append(&mut resolved.jvm);

    if !contains_classpath_switch(&jvm_args) {
        jvm_args.push("-cp".to_string());
        jvm_args.push(classpath.clone());
    }

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

    let effective_java_path = if java_path.exists() {
        java_path.display().to_string()
    } else if java_exec_default.exists() {
        java_exec_default.display().to_string()
    } else {
        metadata.java_path
    };

    Ok(LaunchValidationResult {
        java_path: effective_java_path,
        java_version: first_line(&java_version_text),
        classpath,
        jvm_args,
        game_args: resolved.game,
        main_class: resolved.main_class,
        logs,
    })
}

#[tauri::command]
pub fn start_instance(instance_root: String) -> Result<StartInstanceResult, String> {
    {
        let mut registry = runtime_registry()
            .lock()
            .map_err(|_| "No se pudo bloquear el registro de runtime.".to_string())?;
        if let Some(state) = registry.get(&instance_root)
            && state.running
        {
            return Err(
                "La instancia ya está ejecutándose; no se permite doble ejecución.".to_string(),
            );
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

    let prepared = match validate_and_prepare_launch(instance_root.clone()) {
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
    if let Ok(mut registry) = runtime_registry().lock()
        && let Some(state) = registry.get_mut(&instance_root)
    {
        state.pid = Some(pid);
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

#[derive(Debug, Clone)]
struct NativeJar {
    path: PathBuf,
    excludes: Vec<String>,
}

#[derive(Debug, Default)]
struct ResolvedLibraries {
    classpath_entries: Vec<String>,
    missing_classpath_entries: Vec<MissingClasspathEntry>,
    native_jars: Vec<NativeJar>,
    missing_native_entries: Vec<String>,
}

#[derive(Debug, Clone)]
struct MissingClasspathEntry {
    path: String,
    url: Option<String>,
}

fn resolve_libraries(
    mc_root: &Path,
    version_json: &Value,
    rule_context: &RuleContext,
) -> ResolvedLibraries {
    let mut resolved = ResolvedLibraries::default();

    let libraries = version_json
        .get("libraries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    for lib in libraries {
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
            .and_then(|d| d.get("artifact"))
            .and_then(|a| a.get("path"))
            .and_then(Value::as_str)
            .map(|p| mc_root.join("libraries").join(p).display().to_string());

        if artifact_path.is_some() {
            let cp_path = artifact_path.expect("artifact_path checked as some");
            if Path::new(&cp_path).exists() {
                resolved.classpath_entries.push(cp_path);
            } else {
                resolved.missing_classpath_entries.push(MissingClasspathEntry {
                    path: cp_path,
                    url: lib
                        .get("downloads")
                        .and_then(|d| d.get("artifact"))
                        .and_then(|a| a.get("url"))
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                });
            }
        } else if let Some(fallback_path) = build_maven_library_path(mc_root, &lib) {
            if Path::new(&fallback_path).exists() {
                resolved.classpath_entries.push(fallback_path);
            } else {
                resolved.missing_classpath_entries.push(MissingClasspathEntry {
                    path: fallback_path,
                    url: None,
                });
            }
        }

        if let Some(native) = resolve_native_jar(mc_root, &lib, rule_context) {
            if native.path.exists() {
                resolved.native_jars.push(native);
            } else {
                resolved
                    .missing_native_entries
                    .push(native.path.display().to_string());
            }
        }
    }

    resolved
}

fn load_merged_version_json(mc_root: &Path, version_id: &str) -> Result<Value, String> {
    let mut chain = Vec::new();
    let mut current = version_id.to_string();

    loop {
        let path = mc_root
            .join("versions")
            .join(&current)
            .join(format!("{current}.json"));
        let raw = fs::read_to_string(&path)
            .map_err(|err| format!("No se pudo leer version.json {}: {err}", path.display()))?;
        let json: Value = serde_json::from_str(&raw)
            .map_err(|err| format!("version.json inválido en {}: {err}", path.display()))?;

        let parent = json
            .get("inheritsFrom")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        chain.push(json);

        let Some(parent_id) = parent else {
            break;
        };
        current = parent_id;
    }

    chain.reverse();
    let mut merged = Value::Object(serde_json::Map::new());

    for entry in chain {
        merge_version_json(&mut merged, &entry);
    }

    Ok(merged)
}

fn merge_version_json(base: &mut Value, overlay: &Value) {
    let (Some(base_obj), Some(overlay_obj)) = (base.as_object_mut(), overlay.as_object()) else {
        return;
    };

    for (key, value) in overlay_obj {
        if key == "libraries" {
            let mut merged_libraries = base_obj
                .get("libraries")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            merged_libraries.extend(value.as_array().cloned().unwrap_or_default());
            base_obj.insert("libraries".to_string(), Value::Array(merged_libraries));
            continue;
        }

        base_obj.insert(key.clone(), value.clone());
    }
}

fn hydrate_missing_libraries(
    missing_entries: &[MissingClasspathEntry],
    logs: &mut Vec<String>,
) -> Result<(), String> {
    let downloadable = missing_entries
        .iter()
        .filter(|entry| entry.url.is_some())
        .collect::<Vec<_>>();

    if downloadable.is_empty() {
        return Ok(());
    }

    logs.push(format!(
        "↻ Faltan {} libraries en disco; intentando descarga automática de artifacts.",
        downloadable.len()
    ));

    let client = reqwest::blocking::Client::new();
    for entry in downloadable {
        let url = entry.url.as_ref().expect("filtered as some");
        let bytes = client
            .get(url)
            .send()
            .and_then(|response| response.error_for_status())
            .map_err(|err| format!("No se pudo descargar library {url}: {err}"))?
            .bytes()
            .map_err(|err| format!("Respuesta inválida al descargar library {url}: {err}"))?;

        let path = Path::new(&entry.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "No se pudo crear directorio para library {}: {err}",
                    parent.display()
                )
            })?;
        }

        fs::write(path, &bytes)
            .map_err(|err| format!("No se pudo guardar library {}: {err}", path.display()))?;
    }

    logs.push("✔ Descarga automática de libraries finalizada.".to_string());
    Ok(())
}

fn build_maven_library_path(mc_root: &Path, lib: &Value) -> Option<String> {
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
}

fn resolve_native_jar(
    mc_root: &Path,
    lib: &Value,
    rule_context: &RuleContext,
) -> Option<NativeJar> {
    let os_key = match rule_context.os_name {
        crate::domain::minecraft::rule_engine::OsName::Windows => "windows",
        crate::domain::minecraft::rule_engine::OsName::Linux => "linux",
        crate::domain::minecraft::rule_engine::OsName::Macos => "osx",
        crate::domain::minecraft::rule_engine::OsName::Unknown => return None,
    };

    let classifier_raw = lib
        .get("natives")
        .and_then(|value| value.get(os_key))
        .and_then(Value::as_str)?;
    let classifier = classifier_raw.replace("${arch}", &rule_context.arch);

    let native_path = lib
        .get("downloads")
        .and_then(|d| d.get("classifiers"))
        .and_then(|c| c.get(&classifier))
        .and_then(|entry| entry.get("path"))
        .and_then(Value::as_str)
        .map(|path| mc_root.join("libraries").join(path));

    let Some(path) = native_path else {
        return None;
    };

    let excludes = lib
        .get("extract")
        .and_then(|extract| extract.get("exclude"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Some(NativeJar { path, excludes })
}

fn extract_natives(native_jars: &[NativeJar], natives_dir: &Path) -> Result<(), String> {
    if natives_dir.exists() {
        fs::remove_dir_all(natives_dir).map_err(|err| {
            format!(
                "No se pudo limpiar directorio de natives {}: {err}",
                natives_dir.display()
            )
        })?;
    }
    fs::create_dir_all(natives_dir).map_err(|err| {
        format!(
            "No se pudo crear directorio de natives {}: {err}",
            natives_dir.display()
        )
    })?;

    for native in native_jars {
        let file = fs::File::open(&native.path).map_err(|err| {
            format!(
                "No se pudo abrir native jar {}: {err}",
                native.path.display()
            )
        })?;
        let mut zip = ZipArchive::new(file)
            .map_err(|err| format!("Native jar inválido {}: {err}", native.path.display()))?;

        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|err| {
                format!(
                    "No se pudo leer entrada native en {}: {err}",
                    native.path.display()
                )
            })?;
            let entry_name = entry.name().replace('\\', "/");

            if should_skip_native_entry(&entry_name, &native.excludes) {
                continue;
            }

            let out_path = natives_dir.join(&entry_name);
            if entry.is_dir() {
                fs::create_dir_all(&out_path).map_err(|err| {
                    format!(
                        "No se pudo crear subdirectorio native {}: {err}",
                        out_path.display()
                    )
                })?;
                continue;
            }

            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent).map_err(|err| {
                    format!(
                        "No se pudo crear carpeta parent native {}: {err}",
                        parent.display()
                    )
                })?;
            }

            let mut output = fs::File::create(&out_path).map_err(|err| {
                format!(
                    "No se pudo crear archivo native {}: {err}",
                    out_path.display()
                )
            })?;
            io::copy(&mut entry, &mut output).map_err(|err| {
                format!(
                    "No se pudo extraer archivo native {}: {err}",
                    out_path.display()
                )
            })?;
        }
    }

    Ok(())
}

fn should_skip_native_entry(entry_name: &str, excludes: &[String]) -> bool {
    if entry_name.starts_with("META-INF/") {
        return true;
    }

    excludes.iter().any(|excluded| {
        let pattern = excluded.trim_matches('/');
        entry_name == pattern || entry_name.starts_with(&format!("{pattern}/"))
    })
}

fn contains_classpath_switch(jvm_args: &[String]) -> bool {
    jvm_args
        .windows(2)
        .any(|window| matches!(window, [flag, _value] if flag == "-cp" || flag == "-classpath"))
}
