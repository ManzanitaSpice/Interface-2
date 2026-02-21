use std::{
    collections::{HashMap, VecDeque},
    fs,
    hash::{Hash, Hasher},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};
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
            rule_engine::{RuleContext, RuleFeatures},
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
    pub refreshed_auth_session: LaunchAuthSession,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartInstanceResult {
    pub pid: u32,
    pub java_path: String,
    pub logs: Vec<String>,
    pub refreshed_auth_session: LaunchAuthSession,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeOutputEvent {
    instance_root: String,
    stream: String,
    line: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub running: bool,
    pub pid: Option<u32>,
    pub exit_code: Option<i32>,
    pub stderr_tail: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShortcutRedirect {
    source_path: String,
    source_launcher: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceCardStats {
    pub size_mb: u64,
    pub mods_count: u32,
    pub last_used: Option<String>,
}

#[derive(Debug, Clone)]
struct RuntimeState {
    pid: Option<u32>,
    running: bool,
    exit_code: Option<i32>,
    stderr_tail: VecDeque<String>,
    started_at: Instant,
}

#[derive(Debug, Clone)]
struct VerifiedLaunchAuth {
    profile_id: String,
    profile_name: String,
    minecraft_access_token: String,
    minecraft_access_token_expires_at: Option<u64>,
    premium_verified: bool,
}

static RUNTIME_REGISTRY: OnceLock<Mutex<HashMap<String, RuntimeState>>> = OnceLock::new();

fn runtime_registry() -> &'static Mutex<HashMap<String, RuntimeState>> {
    RUNTIME_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn has_running_instances() -> Result<bool, String> {
    let registry = runtime_registry()
        .lock()
        .map_err(|_| "No se pudo bloquear el registro de runtime.".to_string())?;
    Ok(registry.values().any(|state| state.running))
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
            stderr_tail: state.stderr_tail.iter().cloned().collect(),
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
pub fn open_redirect_origin_folder(instance_root: String) -> Result<(), String> {
    let redirect_path = Path::new(&instance_root).join(".redirect.json");
    let raw = fs::read_to_string(&redirect_path).map_err(|err| {
        format!(
            "No se pudo leer redirección de atajo en {}: {err}",
            redirect_path.display()
        )
    })?;
    let redirect: ShortcutRedirect = serde_json::from_str(&raw).map_err(|err| {
        format!(
            "No se pudo parsear redirección de atajo en {}: {err}",
            redirect_path.display()
        )
    })?;
    open_instance_folder(redirect.source_path)
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<(), String> {
    if !source.exists() {
        return Err(format!("La carpeta origen no existe: {}", source.display()));
    }

    fs::create_dir_all(destination).map_err(|err| {
        format!(
            "No se pudo crear carpeta destino {}: {err}",
            destination.display()
        )
    })?;

    let entries = fs::read_dir(source)
        .map_err(|err| format!("No se pudo leer carpeta origen {}: {err}", source.display()))?;

    for entry in entries {
        let entry = entry.map_err(|err| format!("No se pudo iterar carpeta origen: {err}"))?;
        let path = entry.path();
        let target = destination.join(entry.file_name());

        if path.is_dir() {
            copy_dir_recursive(&path, &target)?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|err| {
                    format!("No se pudo crear carpeta {}: {err}", parent.display())
                })?;
            }
            fs::copy(&path, &target).map_err(|err| {
                format!(
                    "No se pudo copiar archivo {} -> {}: {err}",
                    path.display(),
                    target.display()
                )
            })?;
        }
    }

    Ok(())
}

fn has_game_markers(path: &Path) -> bool {
    path.join("versions").is_dir()
        || path.join("mods").is_dir()
        || path.join("assets").is_dir()
        || path.join("options.txt").is_file()
        || path.join("saves").is_dir()
}

fn detect_runtime_game_dir(root: &Path) -> Option<PathBuf> {
    let direct_candidates = [root.join("minecraft"), root.join(".minecraft")];
    if let Some(path) = direct_candidates
        .into_iter()
        .find(|candidate| candidate.is_dir())
    {
        return Some(path);
    }

    if has_game_markers(root) {
        return Some(root.to_path_buf());
    }

    let mut best: Option<(u8, PathBuf)> = None;
    let Ok(entries) = fs::read_dir(root) else {
        return None;
    };

    for entry in entries.flatten() {
        let candidate = entry.path();
        if !candidate.is_dir() {
            continue;
        }

        let mut score = 0u8;
        if candidate.join("versions").is_dir() {
            score = score.saturating_add(5);
        }
        if candidate.join("mods").is_dir() {
            score = score.saturating_add(2);
        }
        if candidate.join("assets").is_dir() {
            score = score.saturating_add(2);
        }
        if candidate.join("options.txt").is_file() {
            score = score.saturating_add(1);
        }
        if score == 0 {
            continue;
        }

        if best
            .as_ref()
            .map(|(best_score, _)| score > *best_score)
            .unwrap_or(true)
        {
            best = Some((score, candidate));
        }
    }

    best.map(|(_, path)| path)
}

fn prepare_runtime_instance_root(app: &AppHandle, instance_root: &str) -> Result<String, String> {
    let metadata = get_instance_metadata(instance_root.to_string())?;
    if !metadata.state.eq_ignore_ascii_case("redirect") {
        return Ok(instance_root.to_string());
    }

    let redirect_path = Path::new(instance_root).join(".redirect.json");
    let raw = fs::read_to_string(&redirect_path).map_err(|err| {
        format!(
            "No se pudo leer redirección de atajo en {}: {err}",
            redirect_path.display()
        )
    })?;
    let redirect: ShortcutRedirect = serde_json::from_str(&raw).map_err(|err| {
        format!(
            "No se pudo parsear redirección de atajo en {}: {err}",
            redirect_path.display()
        )
    })?;

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    redirect.source_path.hash(&mut hasher);
    let cache_bucket = format!("shortcut-{:x}", hasher.finish());

    let cache_root = app
        .path()
        .app_cache_dir()
        .map_err(|err| format!("No se pudo resolver cache dir para atajo: {err}"))?
        .join("import-runtime-cache")
        .join(cache_bucket);

    let needs_refresh = !cache_root.exists();
    if needs_refresh {
        fs::create_dir_all(&cache_root)
            .map_err(|err| format!("No se pudo crear cache temporal de atajo: {err}"))?;
        copy_dir_recursive(Path::new(&redirect.source_path), &cache_root)?;
    }

    let target_mc = cache_root.join("minecraft");
    if !target_mc.exists() {
        let Some(detected_game_dir) = detect_runtime_game_dir(&cache_root) else {
            return Err(
                "El atajo no contiene una carpeta de juego válida (minecraft/.minecraft o equivalente)."
                    .to_string(),
            );
        };

        if detected_game_dir != target_mc {
            if detected_game_dir
                .file_name()
                .and_then(|value| value.to_str())
                == Some(".minecraft")
            {
                fs::rename(&detected_game_dir, &target_mc).map_err(|err| {
                    format!(
                        "No se pudo normalizar carpeta .minecraft -> minecraft en cache temporal: {err}"
                    )
                })?;
            } else {
                copy_dir_recursive(&detected_game_dir, &target_mc)?;
            }
        }
    }

    let runtime_metadata = InstanceMetadata {
        name: metadata.name,
        group: metadata.group,
        minecraft_version: metadata.minecraft_version,
        version_id: metadata.version_id,
        loader: metadata.loader,
        loader_version: metadata.loader_version,
        ram_mb: metadata.ram_mb,
        java_args: metadata.java_args,
        java_path: metadata.java_path,
        java_runtime: metadata.java_runtime,
        java_version: metadata.java_version,
        required_java_major: metadata.required_java_major,
        created_at: metadata.created_at,
        state: "REDIRECT_RUNTIME_CACHE".to_string(),
        last_used: metadata.last_used,
        internal_uuid: metadata.internal_uuid,
    };
    let runtime_metadata_path = cache_root.join(".instance.json");
    let runtime_metadata_raw = serde_json::to_string_pretty(&runtime_metadata)
        .map_err(|err| format!("No se pudo serializar metadata runtime de atajo: {err}"))?;
    fs::write(&runtime_metadata_path, runtime_metadata_raw)
        .map_err(|err| format!("No se pudo guardar metadata runtime de atajo: {err}"))?;

    let _ = app.emit(
        "instance_runtime_output",
        RuntimeOutputEvent {
            instance_root: instance_root.to_string(),
            stream: "system".to_string(),
            line: format!(
                "Atajo de {}: runtime temporal {} en {}",
                redirect.source_launcher,
                if needs_refresh {
                    "preparado"
                } else {
                    "reutilizado"
                },
                cache_root.display()
            ),
        },
    );

    Ok(cache_root.display().to_string())
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

fn write_instance_metadata(instance_root: &str, metadata: &InstanceMetadata) -> Result<(), String> {
    let metadata_path = Path::new(instance_root).join(".instance.json");
    let raw = serde_json::to_string_pretty(metadata)
        .map_err(|err| format!("No se pudo serializar metadata de instancia: {err}"))?;
    fs::write(&metadata_path, raw).map_err(|err| {
        format!(
            "No se pudo guardar metadata de la instancia en {}: {err}",
            metadata_path.display()
        )
    })
}

fn touch_instance_last_used(instance_root: &str) -> Result<(), String> {
    let mut metadata = get_instance_metadata(instance_root.to_string())?;
    metadata.last_used = Some(chrono::Utc::now().to_rfc3339());
    write_instance_metadata(instance_root, &metadata)
}

fn folder_size_bytes(root: &Path) -> u64 {
    if !root.exists() {
        return 0;
    }
    let mut total = 0u64;
    let Ok(entries) = fs::read_dir(root) else {
        return 0;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            total = total.saturating_add(folder_size_bytes(&path));
        } else if let Ok(meta) = path.metadata() {
            total = total.saturating_add(meta.len());
        }
    }
    total
}

fn count_mod_files(root: &Path) -> u32 {
    let mods_paths = [
        root.join("minecraft").join("mods"),
        root.join(".minecraft").join("mods"),
        root.join("mods"),
    ];
    let Some(mods_dir) = mods_paths.iter().find(|path| path.is_dir()) else {
        return 0;
    };

    let Ok(entries) = fs::read_dir(mods_dir) else {
        return 0;
    };

    entries
        .flatten()
        .filter_map(|entry| entry.metadata().ok())
        .filter(|meta| meta.is_file())
        .count() as u32
}

#[tauri::command]
pub fn get_instance_card_stats(instance_root: String) -> Result<InstanceCardStats, String> {
    let root_path = PathBuf::from(instance_root.clone());
    let metadata = get_instance_metadata(instance_root)?;

    let effective_root = if metadata.state.eq_ignore_ascii_case("redirect") {
        let redirect_path = root_path.join(".redirect.json");
        let raw = fs::read_to_string(&redirect_path).map_err(|err| {
            format!(
                "No se pudo leer redirección en {}: {err}",
                redirect_path.display()
            )
        })?;
        let redirect: ShortcutRedirect = serde_json::from_str(&raw).map_err(|err| {
            format!(
                "No se pudo parsear redirección en {}: {err}",
                redirect_path.display()
            )
        })?;
        PathBuf::from(redirect.source_path)
    } else {
        root_path
    };

    let size_mb = (folder_size_bytes(&effective_root) / (1024 * 1024)).max(1);
    let mods_count = count_mod_files(&effective_root);

    Ok(InstanceCardStats {
        size_mb,
        mods_count,
        last_used: metadata.last_used,
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

    let mut metadata = get_instance_metadata(instance_root.clone())?;
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
    ensure_loader_ready_for_launch(
        instance_path,
        &mc_root,
        &mut metadata,
        &java_path,
        &mut logs,
    )?;

    let selected_version_id = resolve_effective_version_id(&mc_root, &metadata)?;
    let loader_lower = metadata.loader.trim().to_ascii_lowercase();
    let is_forge = loader_lower == "forge";
    logs.push(format!("VERSION JSON efectivo: {selected_version_id}"));
    let version_json = load_merged_version_json(&mc_root, &selected_version_id)?;
    let forge_generation = if is_forge {
        let detected = detect_forge_generation(&mc_root, &selected_version_id, &version_json);
        logs.push(format!("Forge generación detectada: {:?}", detected));
        detected
    } else {
        ForgeGeneration::Legacy
    };
    log_merged_json_summary(&version_json, &mut logs);
    validate_merged_has_auth_args(&version_json)?;

    let executable_version_id = version_json
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or(&selected_version_id)
        .to_string();
    let vanilla_jar = mc_root
        .join("versions")
        .join(&metadata.minecraft_version)
        .join(format!("{}.jar", &metadata.minecraft_version));

    let loader_jar = mc_root
        .join("versions")
        .join(&executable_version_id)
        .join(format!("{executable_version_id}.jar"));

    let client_jar = if loader_jar.exists() {
        logs.push(format!("✔ usando loader jar: {}", loader_jar.display()));
        loader_jar
    } else if vanilla_jar.exists() {
        logs.push(format!(
            "✔ loader '{}' no genera JAR propio, usando vanilla jar: {}",
            metadata.loader,
            vanilla_jar.display()
        ));
        vanilla_jar
    } else {
        return Err(format!(
            "No se encontró JAR ejecutable.\n\nBuscado loader jar: {}\n\nBuscado vanilla jar: {}",
            loader_jar.display(),
            vanilla_jar.display()
        ));
    };

    logs.push(format!("✔ jar ejecutable: {}", client_jar.display()));

    let resolved_main_class = version_json
        .get("mainClass")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if resolved_main_class.is_empty() {
        return Err("mainClass faltante en version.json efectivo.".to_string());
    }

    let executable_version_json = mc_root
        .join("versions")
        .join(&executable_version_id)
        .join(format!("{executable_version_id}.json"));
    logs.push(format!("MAIN CLASS: {resolved_main_class}"));
    logs.push(format!(
        "VERSION JSON USADO: {}",
        executable_version_json.display()
    ));

    let rule_context = RuleContext::current();
    let resolved_libraries = resolve_libraries(&mc_root, &version_json, &rule_context);

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

    let loader = metadata.loader.trim().to_ascii_lowercase();
    if loader == "vanilla" || loader.is_empty() {
        ensure_main_class_present_in_jar(&client_jar, &resolved_main_class).map_err(|err| {
            format!("{err}. (instancia vanilla, mainClass debe estar en client.jar)")
        })?;
        logs.push(format!(
            "✔ mainClass {resolved_main_class} verificada en client.jar"
        ));
    } else {
        let class_entry = format!("{}.class", resolved_main_class.replace('.', "/"));

        // First try to find the class inside a classpath JAR (works for Fabric, Quilt, legacy Forge).
        let found_in_classpath = resolved_libraries
            .classpath_entries
            .iter()
            .find(|jar_path| {
                std::fs::File::open(jar_path)
                    .ok()
                    .and_then(|file| zip::ZipArchive::new(file).ok())
                    .and_then(|mut archive| archive.by_name(&class_entry).ok().map(|_| true))
                    .unwrap_or(false)
            });

        if let Some(jar_path) = found_in_classpath {
            logs.push(format!(
                "✔ mainClass {resolved_main_class} verificada en library: {}",
                Path::new(jar_path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
            ));
        } else {
            // Modern Forge (≥1.36 approx) loads BootstrapLauncher via the JPMS module path
            // (--module-path JVM arg produced by the installer), NOT via the standard classpath
            // libraries array. The JAR lives in mc_root/libraries but is never added to
            // classpath_entries. Scan the libraries directory on disk as a fallback.
            let main_class_lower = resolved_main_class.to_ascii_lowercase();
            let is_forge_or_neo = loader == "forge" || loader == "neoforge";

            let search_keyword = if main_class_lower.contains("bootstraplauncher")
                || main_class_lower.contains("cpw.mods")
            {
                Some("bootstraplauncher")
            } else if main_class_lower.contains("net.neoforged") {
                Some("neoforged")
            } else {
                None
            };

            let found_in_libraries_dir = is_forge_or_neo
                && search_keyword.map_or(false, |kw| {
                    jar_exists_in_libraries_dir(&mc_root.join("libraries"), kw)
                });

            if found_in_libraries_dir {
                logs.push(format!(
                    "✔ mainClass {resolved_main_class} verificada en libraries dir (módulo JPMS de Forge)"
                ));
            } else {
                let diagnostic = if is_forge_or_neo {
                    format!(
                        "El JAR del launcher ({}) no se encontró en el directorio libraries. \
La instalación de Forge/NeoForge puede estar incompleta.",
                        search_keyword.unwrap_or("bootstraplauncher")
                    )
                } else {
                    format!(
                        "Classpath contiene {} JARs pero ninguno tiene la clase. \
Primeros 5: {}",
                        resolved_libraries.classpath_entries.len(),
                        resolved_libraries
                            .classpath_entries
                            .iter()
                            .take(5)
                            .map(|path| {
                                Path::new(path)
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_string()
                            })
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };

                return Err(format!(
                    "La mainClass '{resolved_main_class}' no se encontró \
en ningún JAR del classpath del loader '{}'.\n{}",
                    metadata.loader, diagnostic
                ));
            }
        }
    }

    let has_bootstrap = resolved_main_class
        .to_ascii_lowercase()
        .contains("bootstraplauncher")
        || resolved_libraries
            .classpath_entries
            .iter()
            .any(|entry| entry.to_ascii_lowercase().contains("bootstraplauncher"))
        // Modern Forge puts BootstrapLauncher on --module-path, not on classpath.
        // Fall back to checking the libraries directory on disk.
        || jar_exists_in_libraries_dir(&mc_root.join("libraries"), "bootstraplauncher");
    logs.push(format!("BOOTSTRAP EN CP: {has_bootstrap}"));

    logs.push(format!("JAVA ejecutado: {}", embedded_java));
    logs.push(format!("versionId efectivo: {selected_version_id}"));
    logs.push(format!("mainClass efectiva: {resolved_main_class}"));
    logs.push(format!(
        "classpath tamaño: {}",
        resolved_libraries.classpath_entries.len() + 1
    ));
    let classpath_preview = resolved_libraries
        .classpath_entries
        .iter()
        .take(5)
        .cloned()
        .collect::<Vec<_>>();
    if classpath_preview.is_empty() {
        logs.push("primeros 5 jars del classpath: (vacío)".to_string());
    } else {
        logs.push(format!(
            "primeros 5 jars del classpath: {}",
            classpath_preview.join(" | ")
        ));
    }

    if loader_lower != "vanilla" && resolved_main_class == "net.minecraft.client.main.Main" {
        return Err(format!(
            "Regla de validación incumplida: loader={} pero mainClass quedó en vanilla ({resolved_main_class}).",
            metadata.loader
        ));
    }
    if let Some(expected_main_class) = expected_main_class_for_loader(&loader_lower, &version_json)
    {
        if resolved_main_class != expected_main_class {
            return Err(format!(
                "Regla de validación incumplida: loader={} requiere mainClass={} pero se obtuvo {}.",
                metadata.loader, expected_main_class, resolved_main_class
            ));
        }
    }
    // Newer NeoForge (21.x+) uses net.neoforged.* instead of cpw.mods.bootstraplauncher
    let has_neoforged_modern = resolved_main_class
        .to_ascii_lowercase()
        .contains("net.neoforged")
        || resolved_libraries
            .classpath_entries
            .iter()
            .any(|e| e.to_ascii_lowercase().contains("net.neoforged"))
        || jar_exists_in_libraries_dir(&mc_root.join("libraries"), "neoforged");
    if loader_lower == "forge"
        && forge_generation == ForgeGeneration::Modern
        && !has_bootstrap
        && !has_neoforged_modern
    {
        return Err(
            "Forge moderno requiere bootstraplauncher en classpath o module-path.".to_string(),
        );
    }
    if loader_lower == "neoforge" && !has_bootstrap && !has_neoforged_modern {
        return Err(format!(
            "Regla de validación incumplida: loader={} requiere bootstraplauncher en classpath.",
            metadata.loader
        ));
    }
    if loader_lower != "vanilla" {
        let effective_version_json = mc_root
            .join("versions")
            .join(&executable_version_id)
            .join(format!("{executable_version_id}.json"));
        let effective_raw = fs::read_to_string(&effective_version_json).map_err(|err| {
            format!(
                "No se pudo leer version.json efectivo para validar inheritsFrom {}: {err}",
                effective_version_json.display()
            )
        })?;
        let effective_json: Value = serde_json::from_str(&effective_raw).map_err(|err| {
            format!(
                "No se pudo parsear version.json efectivo para validar inheritsFrom {}: {err}",
                effective_version_json.display()
            )
        })?;
        if effective_json
            .get("inheritsFrom")
            .and_then(Value::as_str)
            .is_none()
        {
            return Err(format!(
                "Regla de validación incumplida: loader={} requiere inheritsFrom en version.json efectivo.",
                metadata.loader
            ));
        }
    }

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
            .map(|native| PathBuf::from(&native.path))
            .filter(|path| path.exists()),
    );
    validate_jars_as_zip(&jars_to_validate)?;
    logs.push(format!(
        "✔ jars validados como zip: {}",
        jars_to_validate.len()
    ));

    logs.push(format!(
        "native_jars detectados: {}",
        resolved_libraries.native_jars.len()
    ));
    for native in &resolved_libraries.native_jars {
        let file_name = Path::new(&native.path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown");
        logs.push(format!("  - {file_name}"));
    }

    let natives_dir = mc_root.join("natives");
    prepare_natives_dir(&natives_dir)?;
    extract_natives(&resolved_libraries.native_jars, &natives_dir, &mut logs)?;
    log_natives_dir_contents(&natives_dir, &mut logs);
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
    if assets_index.exists() {
        logs.push(format!(
            "✔ assets index presente: {}",
            assets_index.display()
        ));
    } else {
        return Err(format!(
            "Asset index '{}' no encontrado en {}. La instancia puede necesitar re-crearse.",
            assets_index_name,
            assets_index.display()
        ));
    }

    let client_extra = mc_root
        .join("versions")
        .join(&metadata.minecraft_version)
        .join(format!("{}-client-extra.jar", metadata.minecraft_version));
    if !client_extra.exists() {
        logs.push(format!(
            "⚠ client-extra.jar no encontrado: {}. NeoForge puede fallar al cargar recursos de MC.",
            client_extra.display()
        ));
    }

    fs::create_dir_all(mc_root.join("mods"))
        .map_err(|err| format!("No se pudo crear mods/: {err}"))?;

    logs.push("🔹 2. Preparación de ejecución".to_string());

    let sep = if cfg!(target_os = "windows") {
        ";"
    } else {
        ":"
    };
    let mut classpath_entries = resolved_libraries.classpath_entries.clone();
    classpath_entries.push(client_jar.display().to_string());
    verify_no_duplicate_classpath_entries(&classpath_entries, &mut logs)?;
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
        auth_uuid: sanitize_uuid(&verified_auth.profile_id),
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
        clientid: "00000000402b5328".to_string(),
        auth_xuid: extract_xuid_from_jwt(&verified_auth.minecraft_access_token).unwrap_or_default(),
        xuid: extract_xuid_from_jwt(&verified_auth.minecraft_access_token).unwrap_or_default(),
        quick_play_singleplayer: String::new(),
        quick_play_multiplayer: String::new(),
        quick_play_realms: String::new(),
        quick_play_path: String::new(),
    };

    let launch_rules = RuleContext {
        features: RuleFeatures {
            is_demo_user: false,
            has_custom_resolution: false,
            is_quick_play: false,
        },
        ..RuleContext::current()
    };

    let mut resolved = resolve_launch_arguments(&version_json, &launch_context, &launch_rules)?;

    let forge_extra_jvm_args = if is_forge && forge_generation == ForgeGeneration::Modern {
        match load_forge_args_file(&mc_root, &selected_version_id, &launch_context, &mut logs)? {
            Some(args) => args,
            None => {
                return Err(format!(
                    "Forge moderno detectado pero no se encontró win_args.txt/unix_args.txt en versions/{}/. El instalador de Forge debe haber fallado o la instancia debe recrearse.",
                    selected_version_id
                ));
            }
        }
    } else {
        Vec::new()
    };

    let memory_args = vec![
        format!("-Xms{}M", metadata.ram_mb.max(512) / 2),
        format!("-Xmx{}M", metadata.ram_mb.max(512)),
    ];
    let mut jvm_args: Vec<String> = Vec::new();
    jvm_args.extend(memory_args.clone());

    if is_forge && forge_generation == ForgeGeneration::Modern {
        jvm_args.extend(forge_extra_jvm_args.clone());
    }

    jvm_args.extend(
        metadata
            .java_args
            .iter()
            .map(|arg| replace_launch_variables(arg, &launch_context)),
    );
    jvm_args.append(&mut resolved.jvm);

    // Modern Forge (1.17+) needs system properties so its bootstrap can
    // locate libraries and know which JARs to skip mod-scanning.
    // If they are absent from the version.json JVM args, inject them now.
    if loader_lower == "forge" {
        if let Some(fixed_main) = forge_resolve_main_class(
            &resolved.main_class,
            &resolved_libraries.classpath_entries,
            &mut logs,
        ) {
            resolved.main_class = fixed_main;
        }
        forge_inject_system_properties(&mut jvm_args, &mc_root, &mut logs);
    }

    logs.push(format!(
        "DEBUG auth - profile_name: '{}'",
        verified_auth.profile_name
    ));
    logs.push(format!(
        "DEBUG auth - profile_id: '{}'",
        verified_auth.profile_id
    ));
    logs.push(format!(
        "DEBUG auth - token vacío: {}",
        verified_auth.minecraft_access_token.is_empty()
    ));
    logs.push(format!("DEBUG game_args count: {}", resolved.game.len()));
    logs.push(format!("DEBUG game_args completos: {:?}", resolved.game));
    logs.push(format!("DEBUG jvm_args count: {}", jvm_args.len()));
    logs.push(format!(
        "forge_extra_jvm_args count: {}",
        forge_extra_jvm_args.len()
    ));
    let forge_preview = forge_extra_jvm_args
        .iter()
        .take(3)
        .cloned()
        .collect::<Vec<_>>()
        .join(" | ");
    logs.push(format!(
        "Primeros 3 args del file: {}",
        if forge_preview.is_empty() {
            "(sin args file)"
        } else {
            forge_preview.as_str()
        }
    ));

    if !contains_classpath_switch(&jvm_args) {
        jvm_args.push("-cp".to_string());
        jvm_args.push(classpath.clone());
    }

    logs.push(format!(
        "DEBUG java.home — jvm_args completos antes de corrección ({} args): {:?}",
        jvm_args.len(),
        jvm_args
            .iter()
            .filter(|a| a.contains("java.home") || a.contains("module"))
            .collect::<Vec<_>>()
    ));

    // ── Corrección forzada de java.home ────────────────────────────────────
    let java_exec_path = Path::new(&embedded_java);
    let correct_java_home = java_exec_path
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| format!("No se pudo derivar java_home desde: {}", embedded_java))?
        .to_path_buf();

    logs.push(format!(
        "✔ java_home correcto: {}",
        correct_java_home.display()
    ));

    // Corregir cualquier -Djava.home incorrecto en jvm_args
    jvm_args = jvm_args
        .into_iter()
        .map(|arg| {
            if arg.starts_with("-Djava.home=") {
                let corrected = format!("-Djava.home={}", correct_java_home.display());
                if arg != corrected {
                    logs.push(format!("⚠ -Djava.home corregido: {} → {}", arg, corrected));
                }
                corrected
            } else {
                arg
            }
        })
        .collect();

    // Si es Forge y no tiene -Djava.home, agregarlo
    let is_forge_loader = metadata.loader.trim().to_ascii_lowercase() == "forge";
    if is_forge_loader && !jvm_args.iter().any(|a| a.starts_with("-Djava.home=")) {
        let java_home_arg = format!("-Djava.home={}", correct_java_home.display());
        jvm_args.insert(2.min(jvm_args.len()), java_home_arg.clone());
        logs.push(format!(
            "✔ -Djava.home insertado para Forge: {}",
            java_home_arg
        ));
    }

    // Validar que el java.home resultante es válido
    for arg in &jvm_args {
        if let Some(home_str) = arg.strip_prefix("-Djava.home=") {
            let modules = Path::new(home_str).join("lib").join("modules");
            if !modules.exists() {
                return Err(format!(
                    "java_home inválido tras corrección: {}\nlib/modules no existe.\nRuntime embebido: {}",
                    home_str,
                    correct_java_home.display()
                ));
            }
            logs.push(format!("✔ java.home verificado en: {}", home_str));
            break;
        }
    }
    // ── Fin corrección java.home ────────────────────────────────────────────

    logs.push(format!(
        "jvm_args orden final: [memory({})] [forge_file({})] [user({})] [version_json({})] [cp({})]",
        memory_args.len(),
        if is_forge && forge_generation == ForgeGeneration::Modern {
            forge_extra_jvm_args.len()
        } else {
            0
        },
        metadata.java_args.len(),
        jvm_args.len().saturating_sub(memory_args.len()).saturating_sub(metadata.java_args.len()),
        if contains_classpath_switch(&jvm_args) { 2 } else { 0 }
    ));

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
        return Err("Cuenta sin licencia premium verificada. Lanzamiento bloqueado.".to_string());
    }

    validate_required_online_launch_flags(&resolved.game, &launch_context).map_err(|err| {
        format!(
            "Argumentos críticos de sesión incompletos o inválidos. {err}. Lanzamiento bloqueado para evitar Demo."
        )
    })?;

    let username = find_arg_value(&resolved.game, "--username").unwrap_or_default();
    let uuid = find_arg_value(&resolved.game, "--uuid").unwrap_or_default();
    let access_token = find_arg_value(&resolved.game, "--accessToken").unwrap_or_default();
    let user_type = find_arg_value(&resolved.game, "--userType").unwrap_or_default();
    let version_type = find_arg_value(&resolved.game, "--versionType").unwrap_or_default();

    logs.push("CHECK CRÍTICO: argumentos enviados a Java".to_string());
    logs.push(format!("--username {username}"));
    logs.push(format!("--uuid {uuid}"));
    logs.push(format!("--accessToken {access_token}"));
    logs.push(format!("--userType {user_type}"));
    logs.push(format!("--versionType {version_type}"));
    logs.push(format!("TOKEN: {access_token}"));
    logs.push(format!("UUID: {uuid}"));
    logs.push(format!("USERNAME: {username}"));

    if resolved.game.iter().any(|arg| arg == "--demo") {
        return Err(
            "Se detectó --demo en los argumentos de juego. Lanzamiento bloqueado.".to_string(),
        );
    }

    if username != verified_auth.profile_name {
        return Err(format!(
            "--username no coincide con el perfil oficial validado. esperado={} recibido={}",
            verified_auth.profile_name, username
        ));
    }

    if uuid != sanitize_uuid(&verified_auth.profile_id) {
        return Err(format!(
            "--uuid no coincide byte a byte con profile.id validado. esperado={} recibido={}",
            sanitize_uuid(&verified_auth.profile_id),
            uuid
        ));
    }

    if access_token != verified_auth.minecraft_access_token {
        return Err(
            "--accessToken no coincide con el token activo validado; lanzamiento bloqueado."
                .to_string(),
        );
    }

    let command_preview = std::iter::once(embedded_java.clone())
        .chain(jvm_args.iter().cloned())
        .chain(std::iter::once(resolved.main_class.clone()))
        .chain(resolved.game.iter().cloned())
        .collect::<Vec<_>>()
        .join(" ");
    logs.push(format!("COMANDO FINAL JAVA: {command_preview}"));

    Ok(LaunchValidationResult {
        java_path: embedded_java,
        java_version: first_line(&java_version_text),
        classpath,
        jvm_args,
        game_args: resolved.game,
        main_class: resolved.main_class,
        logs,
        refreshed_auth_session: LaunchAuthSession {
            profile_id: verified_auth.profile_id,
            profile_name: verified_auth.profile_name,
            minecraft_access_token: verified_auth.minecraft_access_token,
            minecraft_access_token_expires_at: verified_auth.minecraft_access_token_expires_at,
            microsoft_refresh_token: auth_session.microsoft_refresh_token,
            premium_verified: verified_auth.premium_verified,
        },
    })
}

#[tauri::command]
pub async fn start_instance(
    app: AppHandle,
    instance_root: String,
    auth_session: LaunchAuthSession,
) -> Result<StartInstanceResult, String> {
    let metadata = get_instance_metadata(instance_root.clone())?;
    let _ = touch_instance_last_used(&instance_root);
    if metadata.state.eq_ignore_ascii_case("redirect") {
        register_runtime_start(instance_root.clone())?;
        let result = crate::app::redirect_launch::launch_redirect_instance(
            app,
            instance_root.clone(),
            auth_session,
        )
        .await;
        match result {
            Ok(started) => {
                register_runtime_pid(&instance_root, started.pid);
                return Ok(started);
            }
            Err(err) => {
                if let Ok(mut registry) = runtime_registry().lock() {
                    registry.remove(&instance_root);
                }
                return Err(err);
            }
        }
    }

    register_runtime_start(instance_root.clone())?;

    let runtime_instance_root = match prepare_runtime_instance_root(&app, &instance_root) {
        Ok(value) => value,
        Err(err) => {
            if let Ok(mut registry) = runtime_registry().lock() {
                registry.remove(&instance_root);
            }
            return Err(err);
        }
    };

    let instance_root_for_prepare = runtime_instance_root.clone();
    let prepared = match tauri::async_runtime::spawn_blocking(move || {
        validate_and_prepare_launch(instance_root_for_prepare, auth_session)
    })
    .await
    .map_err(|err| format!("Falló la tarea de validación/lanzamiento: {err}"))?
    {
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
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .current_dir(Path::new(&runtime_instance_root).join("minecraft"));

    #[cfg(unix)]
    {
        command.process_group(0);
    }

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
    register_runtime_pid(&instance_root, pid);

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let instance_root_for_thread = instance_root.clone();
    let expected_username = prepared.refreshed_auth_session.profile_name.clone();

    let app_for_thread = app.clone();

    thread::spawn(move || {
        let stop_log_monitor = Arc::new(AtomicBool::new(false));
        let monitor_stop_signal = Arc::clone(&stop_log_monitor);
        let monitor_instance = instance_root_for_thread.clone();
        let monitor_username = expected_username.clone();
        let monitor_app = app_for_thread.clone();
        let monitor_handle = thread::spawn(move || {
            monitor_latest_log_for_auth(
                monitor_app,
                monitor_instance,
                monitor_username,
                pid,
                monitor_stop_signal,
            );
        });
        let stderr_tail = Arc::new(Mutex::new(VecDeque::<String>::new()));
        let mut stream_threads = Vec::new();

        if let Some(stdout_pipe) = stdout {
            let instance_for_stdout = instance_root_for_thread.clone();
            let app_for_stdout = app_for_thread.clone();
            let tail_for_stdout = Arc::clone(&stderr_tail);
            stream_threads.push(thread::spawn(move || {
                let reader = BufReader::new(stdout_pipe);
                for line in reader.lines().map_while(Result::ok) {
                    if line.trim().is_empty() {
                        continue;
                    }
                    log::info!("[MC-STDOUT][{}] {}", instance_for_stdout, line);
                    let _ = app_for_stdout.emit(
                        "instance_runtime_output",
                        RuntimeOutputEvent {
                            instance_root: instance_for_stdout.clone(),
                            stream: "stdout".to_string(),
                            line: line.clone(),
                        },
                    );
                    if let Ok(mut tail) = tail_for_stdout.lock() {
                        tail.push_back(format!("[stdout] {line}"));
                        if tail.len() > 200 {
                            tail.pop_front();
                        }
                    }
                }
            }));
        }

        if let Some(stderr_pipe) = stderr {
            let instance_for_stderr = instance_root_for_thread.clone();
            let app_for_stderr = app_for_thread.clone();
            let tail_for_stderr = Arc::clone(&stderr_tail);
            stream_threads.push(thread::spawn(move || {
                let reader = BufReader::new(stderr_pipe);
                for line in reader.lines().map_while(Result::ok) {
                    if line.trim().is_empty() {
                        continue;
                    }
                    log::warn!("[MC-STDERR][{}] {}", instance_for_stderr, line);
                    let _ = app_for_stderr.emit(
                        "instance_runtime_output",
                        RuntimeOutputEvent {
                            instance_root: instance_for_stderr.clone(),
                            stream: "stderr".to_string(),
                            line: line.clone(),
                        },
                    );
                    if let Ok(mut tail) = tail_for_stderr.lock() {
                        tail.push_back(format!("[stderr] {line}"));
                        if tail.len() > 200 {
                            tail.pop_front();
                        }
                    }
                }
            }));
        }

        for handle in stream_threads {
            let _ = handle.join();
        }

        let exit_code = child.wait().ok().and_then(|status| status.code());
        stop_log_monitor.store(true, Ordering::Relaxed);
        let _ = monitor_handle.join();
        let final_tail = stderr_tail
            .lock()
            .map(|tail| tail.clone())
            .unwrap_or_else(|_| VecDeque::new());

        let _ = app_for_thread.emit(
            "instance_runtime_output",
            RuntimeOutputEvent {
                instance_root: instance_root_for_thread.clone(),
                stream: "system".to_string(),
                line: format!(
                    "Proceso finalizado (pid={pid}) con exit_code={}",
                    exit_code
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "desconocido".to_string())
                ),
            },
        );

        let runtime_tail: VecDeque<String> = final_tail
            .into_iter()
            .rev()
            .take(50)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        let _ = app_for_thread.emit(
            "instance_runtime_exit",
            serde_json::json!({
                "instanceRoot": instance_root_for_thread.clone(),
                "exitCode": exit_code,
                "pid": pid,
            }),
        );

        if let Ok(mut registry) = runtime_registry().lock() {
            registry.insert(
                instance_root_for_thread,
                RuntimeState {
                    pid: Some(pid),
                    running: false,
                    exit_code,
                    stderr_tail: runtime_tail,
                    started_at: Instant::now(),
                },
            );
        }
    });

    let java_path = prepared.java_path.clone();

    Ok(StartInstanceResult {
        pid,
        java_path,
        logs: vec![
            "Comando de lanzamiento ejecutado con argumentos validados.".to_string(),
            format!(
                "Comando final ejecutado: {}",
                std::iter::once(prepared.java_path)
                    .chain(prepared.jvm_args.iter().cloned())
                    .chain(std::iter::once(prepared.main_class.clone()))
                    .chain(prepared.game_args.iter().cloned())
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            "Salida estándar y de error conectadas para monitoreo; exit_code persistido al finalizar.".to_string(),
        ],
        refreshed_auth_session: prepared.refreshed_auth_session,
    })
}

fn first_line(text: &str) -> String {
    text.lines()
        .next()
        .unwrap_or("desconocido")
        .trim()
        .to_string()
}

fn now_unix_millis() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis() as u64)
}

fn terminate_process(pid: u32) {
    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status();
    }

    #[cfg(not(target_os = "windows"))]
    {
        let group_id = format!("-{pid}");
        let _ = Command::new("kill").args(["-TERM", &group_id]).status();
        thread::sleep(Duration::from_millis(450));
        let _ = Command::new("kill").args(["-KILL", &group_id]).status();
        let _ = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status();
        let _ = Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .status();
    }
}

pub fn register_runtime_start(instance_root: String) -> Result<(), String> {
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
        instance_root,
        RuntimeState {
            pid: None,
            running: true,
            exit_code: None,
            stderr_tail: VecDeque::new(),
            started_at: Instant::now(),
        },
    );
    Ok(())
}

pub fn register_runtime_pid(instance_root: &str, pid: u32) {
    if let Ok(mut registry) = runtime_registry().lock() {
        if let Some(state) = registry.get_mut(instance_root) {
            state.pid = Some(pid);
        }
    }
}

pub fn register_runtime_exit(instance_root: &str, pid: u32, exit_code: Option<i32>) {
    if let Ok(mut registry) = runtime_registry().lock() {
        registry.insert(
            instance_root.to_string(),
            RuntimeState {
                pid: Some(pid),
                running: false,
                exit_code,
                stderr_tail: VecDeque::new(),
                started_at: Instant::now(),
            },
        );
    }
}

#[tauri::command]
pub fn force_close_instance(instance_root: String) -> Result<String, String> {
    let pid = {
        let mut registry = runtime_registry()
            .lock()
            .map_err(|_| "No se pudo bloquear el registro de runtime.".to_string())?;
        let Some(state) = registry.get_mut(&instance_root) else {
            return Err("No existe estado de ejecución para esta instancia.".to_string());
        };
        if !state.running {
            return Err("La instancia no está en ejecución.".to_string());
        }
        let Some(pid) = state.pid else {
            return Err("La instancia está iniciando y aún no tiene PID asignado.".to_string());
        };
        state.running = false;
        state.exit_code = Some(-9);
        pid
    };

    terminate_process(pid);
    Ok(format!(
        "Se forzó el cierre completo del proceso (PID {pid})."
    ))
}

fn monitor_latest_log_for_auth(
    app: AppHandle,
    instance_root: String,
    expected_username: String,
    pid: u32,
    stop_signal: Arc<AtomicBool>,
) {
    let latest_log_path = Path::new(&instance_root)
        .join("minecraft")
        .join("logs")
        .join("latest.log");

    let started = Instant::now();
    while !stop_signal.load(Ordering::Relaxed) && started.elapsed() < Duration::from_secs(180) {
        if let Ok(content) = fs::read_to_string(&latest_log_path) {
            if content.contains("Setting user: Demo") {
                let _ = app.emit(
                    "instance_runtime_output",
                    RuntimeOutputEvent {
                        instance_root: instance_root.clone(),
                        stream: "system".to_string(),
                        line: "ERROR AUTH: latest.log reportó 'Setting user: Demo'. Se aborta el proceso por autenticación inválida.".to_string(),
                    },
                );
                terminate_process(pid);
                break;
            }

            if content.contains(&expected_username) {
                let _ = app.emit(
                    "instance_runtime_output",
                    RuntimeOutputEvent {
                        instance_root: instance_root.clone(),
                        stream: "system".to_string(),
                        line: format!(
                            "OK AUTH: latest.log contiene el username oficial validado ({expected_username})."
                        ),
                    },
                );
                break;
            }
        }

        thread::sleep(Duration::from_secs(1));
    }
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

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|err| {
            format!("No se pudo construir cliente HTTP para auth de Minecraft: {err}")
        })?;
    let mut active_minecraft_token = auth_session.minecraft_access_token.clone();
    let mut active_minecraft_expires_at = auth_session.minecraft_access_token_expires_at;

    let mut needs_refresh = false;
    if let (Some(expires_at), Some(now)) = (active_minecraft_expires_at, now_unix_millis()) {
        if expires_at <= now.saturating_add(60_000) {
            logs.push(
                "⚠ access_token próximo a expirar; refrescando de forma preventiva (MSA→XBL→XSTS→Minecraft).".to_string(),
            );
            needs_refresh = true;
        }
    }

    logs.push("CHECK obligatorio: validando perfil oficial vía /minecraft/profile".to_string());

    let mut profile_response = if needs_refresh {
        None
    } else {
        Some(
            client
                .get("https://api.minecraftservices.com/minecraft/profile")
                .header(
                    "Authorization",
                    format!("Bearer {}", active_minecraft_token),
                )
                .header("Accept", "application/json")
                .send()
                .map_err(|err| format!("No se pudo consultar perfil de Minecraft: {err}"))?,
        )
    };

    if profile_response
        .as_ref()
        .map(|response| response.status().as_u16() == 401)
        .unwrap_or(false)
        || needs_refresh
    {
        logs.push(
            "⚠ access_token expirado/inválido; intentando refresh oficial Microsoft/Xbox/XSTS..."
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

        let refreshed = runtime.block_on(async {
            let client = reqwest::Client::new();
            let ms = refresh_microsoft_access_token(&client, &refresh_token).await?;
            let xbox = authenticate_with_xbox_live(&client, &ms.access_token).await?;
            let xsts = authorize_xsts(&client, &xbox.token).await?;
            let mc = login_minecraft_with_xbox(&client, &xsts.uhs, &xsts.token).await?;
            let expires_at = mc.expires_in.and_then(|expires_in| {
                now_unix_millis().map(|now| now.saturating_add(expires_in.saturating_mul(1000)))
            });
            Ok::<(String, Option<u64>), String>((mc.access_token, expires_at))
        })?;

        active_minecraft_token = refreshed.0;
        active_minecraft_expires_at = refreshed.1;
        profile_response = Some(
            client
                .get("https://api.minecraftservices.com/minecraft/profile")
                .header(
                    "Authorization",
                    format!("Bearer {}", active_minecraft_token),
                )
                .header("Accept", "application/json")
                .send()
                .map_err(|err| {
                    format!("No se pudo consultar perfil de Minecraft tras refresh: {err}")
                })?,
        );
    }

    let profile_response = profile_response.ok_or_else(|| {
        "No se obtuvo respuesta de perfil de Minecraft tras validación/refresco.".to_string()
    })?;

    let profile_status = profile_response.status();
    logs.push(format!(
        "GET /minecraft/profile -> HTTP {}",
        profile_status.as_u16()
    ));
    if profile_status.as_u16() != 200 {
        let body = profile_response.text().unwrap_or_default();
        return Err(format!(
            "La API de perfil de Minecraft devolvió error HTTP: {profile_status}. Body completo: {body}. Lanzamiento bloqueado."
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

    if profile_id.contains('-') {
        return Err(
            "profile.id devolvió UUID con guiones; se bloquea por requisito de UUID oficial sin guiones."
                .to_string(),
        );
    }

    if profile_id != auth_session.profile_id || profile_name != auth_session.profile_name {
        return Err("El perfil de Minecraft no coincide con la sesión actual; token inválido o vencido. Se bloquea para evitar modo Demo.".to_string());
    }

    logs.push("CHECK obligatorio: validando licencia vía /entitlements/mcstore".to_string());

    let runtime = tokio::runtime::Runtime::new()
        .map_err(|err| format!("No se pudo crear runtime para validar entitlements: {err}"))?;
    let has_license = runtime.block_on(async {
        has_minecraft_license(&reqwest::Client::new(), &active_minecraft_token).await
    })?;

    if !has_license {
        return Err("Cuenta sin licencia premium verificada. Lanzamiento bloqueado.".to_string());
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
        minecraft_access_token_expires_at: active_minecraft_expires_at,
        premium_verified: true,
    })
}

fn find_arg_value(args: &[String], flag: &str) -> Option<String> {
    args.windows(2).find_map(|window| match window {
        [name, value] if name == flag => Some(value.clone()),
        _ => None,
    })
}

fn extract_xuid_from_jwt(token: &str) -> Option<String> {
    let payload_b64 = token.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD.decode(payload_b64).ok()?;
    let payload: Value = serde_json::from_slice(&decoded).ok()?;
    payload
        .get("xuid")
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn sanitize_uuid(uuid: &str) -> String {
    uuid.replace('-', "")
}

fn validate_merged_has_auth_args(merged: &Value) -> Result<(), String> {
    let has_username_placeholder = if merged.get("arguments").is_some() {
        merged
            .get("arguments")
            .and_then(|a| a.get("game"))
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter().any(|entry| {
                    if let Some(text) = entry.as_str() {
                        return text.contains("auth_player_name");
                    }

                    entry
                        .get("value")
                        .map(|value| match value {
                            Value::String(text) => text.contains("auth_player_name"),
                            Value::Array(items) => items.iter().any(|item| {
                                item.as_str()
                                    .map(|text| text.contains("auth_player_name"))
                                    .unwrap_or(false)
                            }),
                            _ => false,
                        })
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    } else {
        merged
            .get("minecraftArguments")
            .and_then(Value::as_str)
            .map(|s| s.contains("auth_player_name"))
            .unwrap_or(false)
    };

    if !has_username_placeholder {
        return Err(
            "El version.json mergeado no contiene el placeholder auth_player_name en los game arguments. El merge puede haber perdido los arguments del parent (vanilla). Verifica la función load_merged_version_json().".to_string()
        );
    }

    Ok(())
}

fn log_merged_json_summary(merged: &serde_json::Value, logs: &mut Vec<String>) {
    let main_class = merged
        .get("mainClass")
        .and_then(|v| v.as_str())
        .unwrap_or("(ausente)");

    let has_modern_args = merged.get("arguments").is_some();
    let has_legacy_args = merged.get("minecraftArguments").is_some();

    let game_args_count = merged
        .get("arguments")
        .and_then(|a| a.get("game"))
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    let jvm_args_count = merged
        .get("arguments")
        .and_then(|a| a.get("jvm"))
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    let libs_count = merged
        .get("libraries")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    let has_username = if has_modern_args {
        merged
            .get("arguments")
            .and_then(|a| a.get("game"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter().any(|v| {
                    v.as_str()
                        .map(|s| s.contains("auth_player_name"))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    } else {
        merged
            .get("minecraftArguments")
            .and_then(|v| v.as_str())
            .map(|s| s.contains("auth_player_name"))
            .unwrap_or(false)
    };

    let asset_index = merged
        .get("assetIndex")
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("(ausente)");

    logs.push("── Resumen version.json mergeado ──────────────".to_string());
    logs.push(format!("  mainClass:          {}", main_class));
    logs.push(format!(
        "  formato args:       {}",
        if has_modern_args {
            "moderno (arguments)"
        } else if has_legacy_args {
            "legacy (minecraftArguments)"
        } else {
            "NINGUNO — ERROR"
        }
    ));
    logs.push(format!("  game args count:    {}", game_args_count));
    logs.push(format!("  jvm args count:     {}", jvm_args_count));
    logs.push(format!("  libraries count:    {}", libs_count));
    logs.push(format!("  assetIndex id:      {}", asset_index));
    logs.push(format!("  tiene auth_player_name: {}", has_username));
    logs.push("────────────────────────────────────────────────".to_string());

    if !has_username {
        logs.push(
            "  ⚠ ADVERTENCIA: auth_player_name no encontrado en game args tras el merge. El launch fallará."
                .to_string(),
        );
    }

    if game_args_count == 0 && !has_legacy_args {
        logs.push(
            "  ⚠ ADVERTENCIA: game_args_count es 0 y no hay minecraftArguments. El version.json mergeado está vacío de argumentos de juego."
                .to_string(),
        );
    }
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum ForgeGeneration {
    Legacy,
    Transitional,
    Modern,
}

fn detect_forge_generation(
    mc_root: &Path,
    version_id: &str,
    merged_json: &Value,
) -> ForgeGeneration {
    if merged_json.get("minecraftArguments").is_some() {
        return ForgeGeneration::Legacy;
    }

    let has_args_file = ["win_args.txt", "unix_args.txt"].iter().any(|filename| {
        mc_root
            .join("versions")
            .join(version_id)
            .join(filename)
            .exists()
    });

    if has_args_file {
        let filename = if cfg!(target_os = "windows") {
            "win_args.txt"
        } else {
            "unix_args.txt"
        };
        let path = mc_root.join("versions").join(version_id).join(filename);
        if let Ok(content) = fs::read_to_string(path) {
            if content.contains("--module-path") || content.contains("--add-modules") {
                return ForgeGeneration::Modern;
            }
        }
        return ForgeGeneration::Transitional;
    }

    ForgeGeneration::Transitional
}

fn load_forge_args_file(
    mc_root: &Path,
    version_id: &str,
    launch_context: &LaunchContext,
    logs: &mut Vec<String>,
) -> Result<Option<Vec<String>>, String> {
    let filename = if cfg!(target_os = "windows") {
        "win_args.txt"
    } else {
        "unix_args.txt"
    };

    let path = mc_root.join("versions").join(version_id).join(filename);
    logs.push(format!(
        "Args file path: {} → {}",
        path.display(),
        if path.exists() { "existe" } else { "NO EXISTE" }
    ));

    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&path)
        .map_err(|e| format!("No se pudo leer {}: {e}", path.display()))?;

    let mut args: Vec<String> = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with("--") || line.starts_with('-') {
            if let Some(space_pos) = line.find(' ') {
                let flag = &line[..space_pos];
                let value = line[space_pos + 1..].trim();
                args.push(flag.to_string());
                args.push(replace_launch_variables(value, launch_context));
            } else {
                args.push(replace_launch_variables(line, launch_context));
            }
        } else {
            args.push(replace_launch_variables(line, launch_context));
        }
    }

    let module_path_present = args
        .windows(2)
        .any(|window| matches!(window, [flag, _] if flag == "--module-path"));
    logs.push(format!(
        "✔ Forge args file cargado: {} ({} args, --module-path: {})",
        path.display(),
        args.len(),
        if module_path_present {
            "✔"
        } else {
            "✗ FALTA"
        }
    ));

    if !module_path_present {
        logs.push(
            "⚠ args file existe pero no contiene --module-path. Puede ser Forge transitional."
                .to_string(),
        );
    }

    Ok(Some(args))
}

fn validate_required_online_launch_flags(
    game_args: &[String],
    launch_context: &LaunchContext,
) -> Result<(), String> {
    if game_args.is_empty() {
        return Err(format!(
            "game_args está completamente vacío. El version.json no produjo argumentos de juego. Verifica que el version.json fue mergeado correctamente y que extract_game_args() no retornó Vec vacío. auth_player_name en contexto: '{}'",
            launch_context.auth_player_name
        ));
    }

    let username = find_arg_value(game_args, "--username");
    if username.is_none() {
        let has_unresolved = game_args.iter().any(|a| a.contains("auth_player_name"));
        let diagnostic = if has_unresolved {
            "La variable ${auth_player_name} está presente pero NO fue sustituida. El LaunchContext.auth_player_name probablemente está vacío o replace_launch_variables() no reconoce esa variable."
        } else {
            "--username no está en game_args ni como variable. El merge del version.json perdió los arguments.game del parent vanilla."
        };

        return Err(format!(
            "Falta --username en game_args. Diagnóstico: {} auth_player_name en contexto: '{}' Primeros 10 game_args: {:?}",
            diagnostic,
            launch_context.auth_player_name,
            game_args.iter().take(10).collect::<Vec<_>>()
        ));
    }

    let username = username.unwrap_or_default();
    if username.trim().is_empty() {
        return Err(
            "--username está presente pero vacío. verified_auth.profile_name estaba vacío al construir LaunchContext. Verifica que validate_official_minecraft_auth() retornó un profile_name válido antes de construir LaunchContext.".to_string()
        );
    }

    let uuid = find_arg_value(game_args, "--uuid")
        .ok_or_else(|| "Falta --uuid en game_args".to_string())?;
    if uuid.trim().is_empty() {
        return Err(
            "--uuid está presente pero vacío. verified_auth.profile_id estaba vacío.".to_string(),
        );
    }
    if uuid.contains('-') {
        return Err(format!(
            "--uuid contiene guiones: '{}'. Debe enviarse sin guiones. Aplicar sanitize_uuid() al construir LaunchContext.",
            uuid
        ));
    }

    let token = find_arg_value(game_args, "--accessToken")
        .ok_or_else(|| "Falta --accessToken en game_args".to_string())?;
    if token.trim().is_empty() {
        return Err(
            "--accessToken está presente pero vacío. El minecraft_access_token estaba vacío."
                .to_string(),
        );
    }

    let user_type = find_arg_value(game_args, "--userType");
    if let Some(user_type) = user_type {
        if user_type != "msa" {
            return Err(format!(
                "--userType debe ser msa para evitar Demo, recibido: {user_type}"
            ));
        }
    }

    let version_type = find_arg_value(game_args, "--versionType");
    if let Some(version_type) = version_type {
        if version_type != "release"
            && version_type != "old_alpha"
            && version_type != "old_beta"
            && version_type != "snapshot"
        {
            return Err(format!(
                "--versionType inválido para lanzamiento oficial: {version_type}"
            ));
        }
    }

    Ok(())
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

fn resolve_effective_version_id(
    mc_root: &Path,
    metadata: &InstanceMetadata,
) -> Result<String, String> {
    let explicit_version_id = metadata.version_id.trim();
    if !explicit_version_id.is_empty() {
        return Ok(explicit_version_id.to_string());
    }

    let base = metadata.minecraft_version.trim();
    let loader = metadata.loader.trim().to_ascii_lowercase();
    let loader_version = metadata.loader_version.trim().to_ascii_lowercase();

    if loader == "vanilla" || loader.is_empty() {
        return Ok(base.to_string());
    }

    let versions_dir = mc_root.join("versions");
    let mut candidates = Vec::new();
    if versions_dir.exists() {
        for entry in fs::read_dir(&versions_dir)
            .map_err(|err| format!("No se pudo leer versions {}: {err}", versions_dir.display()))?
        {
            let entry = entry.map_err(|err| format!("No se pudo iterar versions: {err}"))?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(id) = path
                .file_name()
                .and_then(|v| v.to_str())
                .map(ToString::to_string)
            else {
                continue;
            };
            let id_lower = id.to_ascii_lowercase();
            if !id_lower.contains(&loader) {
                continue;
            }
            if !loader_version.is_empty() && !id_lower.contains(&loader_version) {
                continue;
            }
            let version_json_path = versions_dir.join(&id).join(format!("{id}.json"));
            if !version_json_path.exists() {
                continue;
            }
            let raw = fs::read_to_string(&version_json_path).map_err(|err| {
                format!(
                    "No se pudo leer version.json candidato {}: {err}",
                    version_json_path.display()
                )
            })?;
            let parsed: Value = serde_json::from_str(&raw).map_err(|err| {
                format!(
                    "No se pudo parsear version.json candidato {}: {err}",
                    version_json_path.display()
                )
            })?;
            let inherits = parsed
                .get("inheritsFrom")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            let score = if inherits == base.to_ascii_lowercase() {
                3
            } else if inherits.contains(&base.to_ascii_lowercase()) {
                2
            } else {
                1
            };
            candidates.push((score, id));
        }
    }

    if candidates.is_empty() {
        return Ok(base.to_string());
    }

    candidates.sort_by(|a, b| a.cmp(b));
    Ok(candidates
        .pop()
        .map(|(_, id)| id)
        .unwrap_or_else(|| base.to_string()))
}

fn load_single_version_json(mc_root: &Path, version_id: &str) -> Result<serde_json::Value, String> {
    let path = mc_root
        .join("versions")
        .join(version_id)
        .join(format!("{version_id}.json"));

    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("No se pudo leer version.json '{}': {}", path.display(), e))?;

    serde_json::from_str(&raw).map_err(|e| {
        format!(
            "No se pudo parsear version.json '{}': {}",
            path.display(),
            e
        )
    })
}

fn extract_maven_key(lib: &Value) -> Option<String> {
    let name = lib.get("name")?.as_str()?;
    let parts: Vec<&str> = name.splitn(4, ':').collect();

    match parts.len() {
        3 => Some(format!("{}:{}", parts[0], parts[1])),
        4 => Some(format!("{}:{}:{}", parts[0], parts[1], parts[3])),
        _ => Some(name.to_string()),
    }
}

fn merge_version_jsons(parent: serde_json::Value, child: serde_json::Value) -> serde_json::Value {
    use serde_json::{Map, Value};

    let mut result: Map<String, Value> = parent.as_object().cloned().unwrap_or_default();

    let child_obj: Map<String, Value> = match child.as_object() {
        Some(o) => o.clone(),
        None => return Value::Object(result),
    };

    for (key, child_val) in child_obj {
        match key.as_str() {
            "inheritsFrom" => {}
            "libraries" => {
                let parent_libs = result
                    .get("libraries")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let child_libs = child_val.as_array().cloned().unwrap_or_default();

                let mut deduped = Vec::with_capacity(child_libs.len() + parent_libs.len());
                let mut seen_keys = std::collections::HashSet::new();
                let mut fallback_idx = 0usize;

                for lib in &child_libs {
                    let key = extract_maven_key(lib).unwrap_or_else(|| {
                        let key = format!("__unknown_{fallback_idx}");
                        fallback_idx += 1;
                        key
                    });

                    if seen_keys.insert(key) {
                        deduped.push(lib.clone());
                    }
                }

                for lib in &parent_libs {
                    let key = extract_maven_key(lib).unwrap_or_else(|| {
                        let key = format!("__unknown_{fallback_idx}");
                        fallback_idx += 1;
                        key
                    });

                    if seen_keys.insert(key) {
                        deduped.push(lib.clone());
                    }
                }

                result.insert("libraries".to_string(), Value::Array(deduped));
            }
            "arguments" => {
                let parent_arguments = result
                    .get("arguments")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();

                let child_arguments = match child_val.as_object() {
                    Some(o) => o.clone(),
                    None => {
                        continue;
                    }
                };

                let mut merged_arguments = parent_arguments.clone();

                {
                    let parent_game = parent_arguments
                        .get("game")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    let child_game = child_arguments
                        .get("game")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();

                    let mut merged_game = parent_game;
                    merged_game.extend(child_game);
                    merged_arguments.insert("game".to_string(), Value::Array(merged_game));
                }

                {
                    let parent_jvm = parent_arguments
                        .get("jvm")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    let child_jvm = child_arguments
                        .get("jvm")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();

                    let mut merged_jvm = parent_jvm;
                    merged_jvm.extend(child_jvm);
                    merged_arguments.insert("jvm".to_string(), Value::Array(merged_jvm));
                }

                result.insert("arguments".to_string(), Value::Object(merged_arguments));
            }
            "assetIndex" | "assets" | "downloads" => {
                if !result.contains_key(&key) {
                    result.insert(key, child_val);
                }
            }
            "javaVersion" => {
                let parent_major = result
                    .get("javaVersion")
                    .and_then(|v| v.get("majorVersion"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let child_major = child_val
                    .get("majorVersion")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);

                if child_major > parent_major {
                    result.insert("javaVersion".to_string(), child_val);
                }
            }
            "minecraftArguments" => {
                result.insert(key, child_val);
            }
            _ => {
                result.insert(key, child_val);
            }
        }
    }

    Value::Object(result)
}

pub fn load_merged_version_json(
    mc_root: &Path,
    version_id: &str,
) -> Result<serde_json::Value, String> {
    let child = load_single_version_json(mc_root, version_id)?;

    let parent_id = match child.get("inheritsFrom").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => {
            return Ok(child);
        }
    };

    let parent = load_merged_version_json(mc_root, &parent_id).map_err(|e| {
        format!(
            "No se pudo cargar parent '{}' requerido por '{}': {}",
            parent_id, version_id, e
        )
    })?;

    Ok(merge_version_jsons(parent, child))
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

/// Recursively scans `dir` for any `.jar` file whose path (lowercased) contains `keyword`.
/// Used to detect Forge/NeoForge JARs that live in `libraries/` but are launched via
/// --module-path rather than being listed in the version.json `libraries` array.
fn jar_exists_in_libraries_dir(dir: &Path, keyword: &str) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if jar_exists_in_libraries_dir(&path, keyword) {
                return true;
            }
        } else if path
            .to_string_lossy()
            .to_ascii_lowercase()
            .contains(keyword)
            && path.extension().and_then(|e| e.to_str()) == Some("jar")
        {
            return true;
        }
    }
    false
}

fn forge_resolve_main_class(
    current_main_class: &str,
    classpath_entries: &[String],
    logs: &mut Vec<String>,
) -> Option<String> {
    let current_main_lower = current_main_class.to_ascii_lowercase();

    if current_main_lower.contains("bootstraplauncher")
        || current_main_lower.contains("net.neoforged")
    {
        return None;
    }

    let has_bootstrap = classpath_entries
        .iter()
        .any(|entry| entry.to_ascii_lowercase().contains("bootstraplauncher"));
    if has_bootstrap {
        logs.push(
            "Forge detectado con bootstraplauncher en classpath: corrigiendo mainClass a cpw.mods.bootstraplauncher.BootstrapLauncher"
                .to_string(),
        );
        return Some("cpw.mods.bootstraplauncher.BootstrapLauncher".to_string());
    }

    None
}

fn forge_inject_system_properties(
    jvm_args: &mut Vec<String>,
    mc_root: &Path,
    logs: &mut Vec<String>,
) {
    let java_home_value = mc_root.join("java").display().to_string();
    let java_home_key = ["java", "home"].join(".");
    let properties = vec![
        (
            "legacyClassPath",
            mc_root.join("libraries").display().to_string(),
        ),
        (
            "libraryDirectory",
            mc_root.join("libraries").display().to_string(),
        ),
        (
            "ignoreList",
            "bootstraplauncher,securejarhandler".to_string(),
        ),
        (java_home_key.as_str(), java_home_value),
    ];

    for (key, value) in properties {
        let prefix = format!("-D{key}=");
        if !jvm_args.iter().any(|arg| arg.starts_with(&prefix)) {
            jvm_args.push(format!("{prefix}{value}"));
            logs.push(format!("Forge JVM prop inyectada: {key}"));
        }
    }
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
                classpath_entries.push(path.clone());

                let filename = Path::new(&path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("")
                    .to_string();

                let needs_extraction = lib.get("natives").is_some()
                    || (is_native_jar_path(&path) && should_extract_for_platform(&filename));

                if needs_extraction {
                    native_jars.push(NativeJarEntry { path });
                }
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
                    classpath_entries.push(path.clone());
                    let filename = Path::new(&path)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("")
                        .to_string();
                    if should_extract_for_platform(&filename) {
                        native_jars.push(NativeJarEntry { path });
                    }
                }
                Some(path) => missing_native_entries.push(path),
                None => missing_native_entries.push(format!(
                    "native no encontrado para {} ({native_key})",
                    lib.get("name").and_then(Value::as_str).unwrap_or("unknown")
                )),
            }
        }
    }

    let mut seen_paths: std::collections::HashSet<String> = std::collections::HashSet::new();
    classpath_entries.retain(|path| {
        let normalized = path.replace('/', std::path::MAIN_SEPARATOR_STR);
        seen_paths.insert(normalized)
    });

    let mut seen_natives: std::collections::HashSet<String> = std::collections::HashSet::new();
    native_jars.retain(|entry| {
        let normalized = entry.path.replace('/', std::path::MAIN_SEPARATOR_STR);
        seen_natives.insert(normalized)
    });

    ResolvedLibraries {
        classpath_entries,
        missing_classpath_entries,
        native_jars,
        missing_native_entries,
    }
}

fn verify_no_duplicate_classpath_entries(
    classpath_entries: &[String],
    logs: &mut Vec<String>,
) -> Result<(), String> {
    use std::collections::{HashMap, HashSet};

    let mut counts: HashMap<String, usize> = HashMap::new();

    for path in classpath_entries {
        let normalized = path
            .replace('/', std::path::MAIN_SEPARATOR_STR)
            .to_ascii_lowercase();
        *counts.entry(normalized).or_insert(0) += 1;
    }

    let duplicates: Vec<&String> = classpath_entries
        .iter()
        .filter(|path| {
            let normalized = path
                .replace('/', std::path::MAIN_SEPARATOR_STR)
                .to_ascii_lowercase();
            counts.get(&normalized).copied().unwrap_or(0) > 1
        })
        .collect();

    if duplicates.is_empty() {
        logs.push(format!(
            "✔ Classpath verificado: {} entradas, sin duplicados.",
            classpath_entries.len()
        ));
        return Ok(());
    }

    let mut unique_dupes: HashSet<String> = HashSet::new();
    for path in &duplicates {
        let filename = std::path::Path::new(path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        unique_dupes.insert(filename);
    }

    Err(format!(
        "Classpath contiene {} entradas duplicadas que causarán \\n         'Duplicate key' en BootstrapLauncher de NeoForge/Forge.\n\n\
         JARs duplicados: {}\n\n\
         Causa: merge_version_jsons() no deduplicó libraries correctamente.",
        duplicates.len(),
        unique_dupes.into_iter().collect::<Vec<_>>().join(", ")
    ))
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

fn is_native_jar_path(jar_path: &str) -> bool {
    let filename = Path::new(jar_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    filename.contains("-natives-")
}

fn should_extract_for_platform(filename: &str) -> bool {
    let is_windows = cfg!(target_os = "windows");
    let is_linux = cfg!(target_os = "linux");
    let is_macos = cfg!(target_os = "macos");
    let is_x86_64 = std::env::consts::ARCH == "x86_64";
    let is_aarch64 = std::env::consts::ARCH == "aarch64";

    if filename.contains("natives-windows") {
        if !is_windows {
            return false;
        }
        if filename.contains("arm64") && !is_aarch64 {
            return false;
        }
        if filename.contains("windows-x86") && is_x86_64 {
            return false;
        }
        return true;
    }

    if filename.contains("natives-linux") {
        if !is_linux {
            return false;
        }
        if filename.contains("arm64") && !is_aarch64 {
            return false;
        }
        if filename.contains("arm32") && is_x86_64 {
            return false;
        }
        return true;
    }

    if filename.contains("natives-macos") || filename.contains("natives-osx") {
        if !is_macos {
            return false;
        }
        if filename.contains("arm64") && !is_aarch64 {
            return false;
        }
        return true;
    }

    true
}

fn prepare_natives_dir(natives_dir: &Path) -> Result<(), String> {
    if natives_dir.exists() {
        for entry in fs::read_dir(natives_dir)
            .map_err(|err| format!("No se pudo leer natives dir: {err}"))?
            .flatten()
        {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let ext = path
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("");
            let filename = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");

            if matches!(ext, "dll" | "so" | "dylib" | "jnilib") || filename.contains(".so.") {
                fs::remove_file(&path).map_err(|err| {
                    format!("No se pudo limpiar native {}: {err}", path.display())
                })?;
            }
        }
    }

    fs::create_dir_all(natives_dir).map_err(|err| format!("No se pudo crear natives dir: {err}"))
}

fn extract_natives(
    native_jars: &[NativeJarEntry],
    natives_dir: &Path,
    logs: &mut Vec<String>,
) -> Result<(), String> {
    if natives_dir.exists() {
        for entry in fs::read_dir(natives_dir)
            .map_err(|err| format!("Error leyendo natives dir: {err}"))?
            .flatten()
        {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("");
            if matches!(ext, "dll" | "so" | "dylib" | "jnilib") {
                let _ = fs::remove_file(&path);
            }
        }
    }

    fs::create_dir_all(natives_dir).map_err(|err| format!("No se pudo crear natives/: {err}"))?;

    if native_jars.is_empty() {
        return Err("native_jars está vacío. lwjgl.dll no será extraído.

             Causa probable: extract_maven_key() eliminó los JARs 
             natives-windows por colisión de key con el JAR principal.

             Verifica que extract_maven_key() usa el classifier en la key."
            .to_string());
    }

    logs.push(format!(
        "Extrayendo natives de {} JARs → {}",
        native_jars.len(),
        natives_dir.display()
    ));
    for native in native_jars {
        let file_name = Path::new(&native.path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown");
        logs.push(format!("  JAR a extraer: {file_name}"));
    }

    let mut extracted = 0_u32;

    for native in native_jars {
        let jar_path = Path::new(&native.path);
        if !jar_path.exists() {
            logs.push(format!("  ⚠ No existe: {}", native.path));
            continue;
        }

        let file = fs::File::open(jar_path)
            .map_err(|err| format!("No se pudo abrir {}: {err}", native.path))?;
        let mut archive =
            ZipArchive::new(file).map_err(|err| format!("ZIP inválido {}: {err}", native.path))?;

        for i in 0..archive.len() {
            let mut entry = archive
                .by_index(i)
                .map_err(|err| format!("Error en entrada {i}: {err}"))?;

            let name = entry.name().to_string();
            if entry.is_dir() || name.starts_with("META-INF/") {
                continue;
            }

            let ext = Path::new(&name)
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("");
            if !matches!(ext, "dll" | "so" | "dylib" | "jnilib") {
                continue;
            }

            let out_name = Path::new(&name)
                .file_name()
                .and_then(|file_name| file_name.to_str())
                .unwrap_or("")
                .to_string();
            if out_name.is_empty() {
                continue;
            }

            let out_path = natives_dir.join(&out_name);
            let mut out_file = fs::File::create(&out_path)
                .map_err(|err| format!("No se pudo crear {}: {err}", out_path.display()))?;

            std::io::copy(&mut entry, &mut out_file)
                .map_err(|err| format!("Error extrayendo {out_name}: {err}"))?;

            extracted += 1;
            logs.push(format!("  ✓ Extraído: {out_name}"));
        }
    }

    logs.push(format!("✔ Total extraídos: {} archivos nativos", extracted));

    #[cfg(target_os = "windows")]
    {
        let lwjgl_dll = natives_dir.join("lwjgl.dll");
        if !lwjgl_dll.exists() {
            return Err(format!(
                "lwjgl.dll no fue extraído en {}.

                 Archivos en natives/: {:?}

                 JARs procesados: {:?}",
                natives_dir.display(),
                list_dir_files(natives_dir),
                native_jars
                    .iter()
                    .map(|native| native.path.clone())
                    .collect::<Vec<_>>()
            ));
        }
    }

    Ok(())
}

fn list_dir_files(dir: &Path) -> Vec<String> {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default()
}

fn log_natives_dir_contents(natives_dir: &Path, logs: &mut Vec<String>) {
    match fs::read_dir(natives_dir) {
        Ok(entries) => {
            let files: Vec<String> = entries
                .flatten()
                .filter_map(|entry| entry.file_name().into_string().ok())
                .filter(|name| {
                    name.ends_with(".dll")
                        || name.ends_with(".so")
                        || name.ends_with(".dylib")
                        || name.ends_with(".jnilib")
                        || name.contains(".so.")
                })
                .collect();

            if files.is_empty() {
                logs.push(format!(
                    "⚠ natives/ está vacío en {}. LWJGL no encontrará sus DLLs.",
                    natives_dir.display()
                ));
            } else {
                let preview = files.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
                let suffix = if files.len() > 5 {
                    format!(" (+{} más)", files.len() - 5)
                } else {
                    String::new()
                };
                logs.push(format!(
                    "✔ natives/ contiene {} bibliotecas: {preview}{suffix}",
                    files.len()
                ));
            }
        }
        Err(err) => logs.push(format!(
            "⚠ No se pudo leer natives dir {}: {err}",
            natives_dir.display()
        )),
    }
}

fn expected_main_class_for_loader(
    loader: &str,
    version_json: &serde_json::Value,
) -> Option<&'static str> {
    match loader.trim().to_ascii_lowercase().as_str() {
        "vanilla" | "" => Some("net.minecraft.client.main.Main"),
        "fabric" => Some("net.fabricmc.loader.impl.launch.knot.KnotClient"),
        "quilt" => Some("org.quiltmc.loader.impl.launch.knot.KnotClient"),
        "forge" => {
            let has_legacy_args = version_json.get("minecraftArguments").is_some();
            if has_legacy_args {
                return Some("net.minecraft.launchwrapper.Launch");
            }
            None
        }
        _ => None,
    }
}

fn ensure_loader_ready_for_launch(
    _instance_path: &Path,
    mc_root: &Path,
    metadata: &mut InstanceMetadata,
    _java_exec: &Path,
    logs: &mut Vec<String>,
) -> Result<(), String> {
    let loader = metadata.loader.trim().to_ascii_lowercase();
    if loader.is_empty() || loader == "vanilla" {
        return Ok(());
    }

    let current_version_id = metadata.version_id.trim();
    if current_version_id.is_empty() {
        return Err(format!(
            "La instancia usa loader {} pero no tiene versionId efectivo en metadata.",
            metadata.loader
        ));
    }

    let existing_version_json = mc_root
        .join("versions")
        .join(current_version_id)
        .join(format!("{current_version_id}.json"));
    if !existing_version_json.exists() {
        return Err(format!(
            "Loader {} no preparado: falta {}. La instalación debe ocurrir en creación, no en launch.",
            metadata.loader,
            existing_version_json.display()
        ));
    }

    logs.push(format!(
        "✔ Loader {} verificado para launch (sin instalación diferida): versionId={}",
        metadata.loader, current_version_id
    ));

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
    if normalized.contains("shortcut") || normalized.contains("import") {
        return Some(guess_runtime_from_minecraft_version(
            &metadata.minecraft_version,
        ));
    }
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

fn guess_runtime_from_minecraft_version(version: &str) -> JavaRuntime {
    let mut parts = version.split('.');
    let _major = parts
        .next()
        .and_then(|item| item.parse::<u32>().ok())
        .unwrap_or(1);
    let minor = parts
        .next()
        .and_then(|item| item.parse::<u32>().ok())
        .unwrap_or(20);
    if minor >= 20 {
        JavaRuntime::Java21
    } else if minor >= 17 {
        JavaRuntime::Java17
    } else {
        JavaRuntime::Java8
    }
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
        build_maven_library_path, contains_classpath_switch, detect_forge_generation,
        extract_maven_key, load_forge_args_file, merge_version_jsons, parse_runtime_from_metadata,
        parse_runtime_major, should_extract_for_platform, verify_no_duplicate_classpath_entries,
        ForgeGeneration,
    };
    use crate::domain::minecraft::argument_resolver::LaunchContext;
    use crate::domain::models::{instance::InstanceMetadata, java::JavaRuntime};
    use serde_json::json;
    use std::{
        fs,
        path::Path,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn test_temp_dir(prefix: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("{prefix}-{nonce}"));
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn launch_context_for_tests() -> LaunchContext {
        LaunchContext {
            classpath: "cp".to_string(),
            classpath_separator: ":".to_string(),
            library_directory: "/libraries".to_string(),
            natives_dir: "/natives".to_string(),
            launcher_name: "Interface-2".to_string(),
            launcher_version: "0.0.0".to_string(),
            auth_player_name: "player".to_string(),
            auth_uuid: "uuid".to_string(),
            auth_access_token: "token".to_string(),
            user_type: "msa".to_string(),
            user_properties: "{}".to_string(),
            version_name: "1.20.1".to_string(),
            game_directory: "/game".to_string(),
            assets_root: "/assets".to_string(),
            assets_index_name: "17".to_string(),
            version_type: "release".to_string(),
            resolution_width: "854".to_string(),
            resolution_height: "480".to_string(),
            clientid: "cid".to_string(),
            auth_xuid: "xuid".to_string(),
            xuid: "xuid".to_string(),
            quick_play_singleplayer: String::new(),
            quick_play_multiplayer: String::new(),
            quick_play_realms: String::new(),
            quick_play_path: String::new(),
        }
    }

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

    #[test]
    fn forge_legacy_detection_via_minecraft_arguments() {
        let root = test_temp_dir("forge-legacy-detect");
        let json = json!({
            "minecraftArguments": "--username ${auth_player_name}",
            "mainClass": "net.minecraft.launchwrapper.Launch"
        });

        assert_eq!(
            detect_forge_generation(&root, "1.12.2-forge", &json),
            ForgeGeneration::Legacy
        );
    }

    #[test]
    fn forge_modern_detection_requires_args_file_with_module_path() {
        let root = test_temp_dir("forge-modern-detect");
        let version_id = "1.20.1-forge-47.3.0";
        let version_dir = root.join("versions").join(version_id);
        fs::create_dir_all(&version_dir).expect("version dir");
        let args_path = if cfg!(target_os = "windows") {
            version_dir.join("win_args.txt")
        } else {
            version_dir.join("unix_args.txt")
        };
        fs::write(
            &args_path,
            "--module-path\n/libraries/mods\n--add-modules\nALL-MODULE-PATH\n",
        )
        .expect("args file");

        let json = json!({"arguments": {"jvm": []}, "mainClass": "cpw.mods.bootstraplauncher.BootstrapLauncher"});
        assert_eq!(
            detect_forge_generation(&root, version_id, &json),
            ForgeGeneration::Modern
        );
    }

    #[test]
    fn forge_args_file_parsing_splits_flag_and_value_correctly() {
        let root = test_temp_dir("forge-args-parse");
        let version_id = "forge-test";
        let version_dir = root.join("versions").join(version_id);
        fs::create_dir_all(&version_dir).expect("version dir");
        let args_path = if cfg!(target_os = "windows") {
            version_dir.join("win_args.txt")
        } else {
            version_dir.join("unix_args.txt")
        };
        fs::write(
            &args_path,
            "--module-path /libraries/one:/libraries/two\n--add-modules\nALL-MODULE-PATH\n",
        )
        .expect("args file");

        let mut logs = Vec::new();
        let parsed =
            load_forge_args_file(&root, version_id, &launch_context_for_tests(), &mut logs)
                .expect("ok")
                .expect("some");

        assert!(
            parsed
                .windows(2)
                .any(|w| matches!(w, [f, _] if f == "--module-path")),
            "--module-path con su valor debe quedar separado"
        );
        assert!(
            parsed
                .windows(2)
                .any(|w| matches!(w, [f, v] if f == "--add-modules" && v == "ALL-MODULE-PATH")),
            "--add-modules debe preservar su valor en la siguiente posición"
        );
    }

    #[test]
    fn jvm_args_order_for_modern_forge_has_module_path_before_cp() {
        let mut jvm_args = vec!["-Xms512M".to_string(), "-Xmx2048M".to_string()];
        jvm_args.extend([
            "--module-path".to_string(),
            "/libraries/modules".to_string(),
            "--add-modules".to_string(),
            "ALL-MODULE-PATH".to_string(),
            "-Djava.library.path=/natives".to_string(),
        ]);
        if !contains_classpath_switch(&jvm_args) {
            jvm_args.push("-cp".to_string());
            jvm_args.push("/classpath".to_string());
        }

        let module_idx = jvm_args
            .iter()
            .position(|arg| arg == "--module-path")
            .expect("module path");
        let cp_idx = jvm_args.iter().position(|arg| arg == "-cp").expect("cp");
        assert!(module_idx < cp_idx, "--module-path debe ir antes de -cp");
    }

    #[test]
    fn merge_concatenates_game_args_not_overrides() {
        let parent = json!({
            "id": "1.21.1",
            "mainClass": "net.minecraft.client.main.Main",
            "arguments": {
                "game": [
                    "--username", "${auth_player_name}",
                    "--uuid",     "${auth_uuid}",
                    "--accessToken", "${auth_access_token}"
                ],
                "jvm": [
                    "-Djava.library.path=${natives_directory}"
                ]
            },
            "libraries": [
                { "name": "com.mojang:minecraft:1.21.1" }
            ],
            "assetIndex": { "id": "17", "url": "https://..." },
            "assets": "17"
        });

        let child = json!({
            "id": "neoforge-21.1.219",
            "inheritsFrom": "1.21.1",
            "mainClass": "cpw.mods.bootstraplauncher.BootstrapLauncher",
            "arguments": {
                "jvm": [
                    "-DignoreList=bootstraplauncher",
                    "-DlibraryDirectory=${library_directory}"
                ]
            },
            "libraries": [
                { "name": "cpw.mods:bootstraplauncher:1.1.2" }
            ]
        });

        let merged = merge_version_jsons(parent, child);

        assert_eq!(
            merged["mainClass"].as_str().unwrap_or_default(),
            "cpw.mods.bootstraplauncher.BootstrapLauncher"
        );

        let game_args = merged["arguments"]["game"]
            .as_array()
            .expect("arguments.game debe existir");
        let has_username = game_args.iter().any(|v| {
            v.as_str()
                .map(|s| s.contains("auth_player_name"))
                .unwrap_or(false)
        });
        assert!(
            has_username,
            "auth_player_name debe estar en game args tras merge"
        );

        let jvm_args = merged["arguments"]["jvm"]
            .as_array()
            .expect("arguments.jvm debe existir");
        assert!(
            jvm_args.len() >= 3,
            "jvm debe tener parent(1) + child(2) = mínimo 3, tiene {}",
            jvm_args.len()
        );

        let libs = merged["libraries"]
            .as_array()
            .expect("libraries debe existir");
        assert_eq!(
            libs.len(),
            2,
            "libraries debe tener 2 (1 parent + 1 child), tiene {}",
            libs.len()
        );

        assert_eq!(
            merged["assetIndex"]["id"].as_str().unwrap_or_default(),
            "17"
        );

        assert!(
            merged.get("inheritsFrom").is_none(),
            "inheritsFrom no debe estar en el JSON mergeado"
        );
    }

    #[test]
    fn merge_legacy_minecraft_arguments_preserved() {
        let parent = json!({
            "id": "1.12.2",
            "mainClass": "net.minecraft.launchwrapper.Launch",
            "minecraftArguments": "--username ${auth_player_name} --uuid ${auth_uuid} --accessToken ${auth_access_token} --userType ${user_type}",
            "libraries": []
        });

        let child = json!({
            "id": "1.12.2-forge-14.23.5.2860",
            "inheritsFrom": "1.12.2",
            "mainClass": "net.minecraft.launchwrapper.Launch",
            "libraries": [
                { "name": "net.minecraftforge:forge:1.12.2-14.23.5.2860" }
            ]
        });

        let merged = merge_version_jsons(parent, child);

        let mc_args = merged["minecraftArguments"]
            .as_str()
            .expect("minecraftArguments debe existir");
        assert!(
            mc_args.contains("auth_player_name"),
            "minecraftArguments debe contener auth_player_name"
        );
    }

    #[test]
    fn merge_child_jvm_args_added_to_parent() {
        let parent = json!({
            "arguments": {
                "game": ["--username", "${auth_player_name}"],
                "jvm": ["-Djava.library.path=${natives_directory}"]
            },
            "libraries": []
        });

        let child = json!({
            "inheritsFrom": "1.21.1",
            "arguments": {
                "jvm": ["-DignoreList=bootstraplauncher"]
            },
            "libraries": []
        });

        let merged = merge_version_jsons(parent, child);
        let jvm = merged["arguments"]["jvm"]
            .as_array()
            .unwrap_or(&vec![])
            .clone();

        let has_natives = jvm.iter().any(|v| {
            v.as_str()
                .map(|s| s.contains("natives_directory"))
                .unwrap_or(false)
        });
        let has_ignore = jvm.iter().any(|v| {
            v.as_str()
                .map(|s| s.contains("ignoreList"))
                .unwrap_or(false)
        });

        assert!(
            has_natives,
            "jvm debe tener arg de parent (natives_directory)"
        );
        assert!(has_ignore, "jvm debe tener arg de child (ignoreList)");
    }
    #[test]
    fn merge_deduplicates_libraries_child_wins() {
        let parent = json!({
            "libraries": [
                { "name": "com.google.code.gson:gson:2.10.1",
                  "downloads": { "artifact": { "path": "gson/gson-2.10.1.jar" } } },
                { "name": "org.slf4j:slf4j-api:2.0.9",
                  "downloads": { "artifact": { "path": "slf4j/slf4j-api-2.0.9.jar" } } },
                { "name": "com.mojang:authlib:6.0.54",
                  "downloads": { "artifact": { "path": "authlib/authlib-6.0.54.jar" } } }
            ]
        });

        let child = json!({
            "inheritsFrom": "1.21.1",
            "libraries": [
                { "name": "com.google.code.gson:gson:2.10.1",
                  "downloads": { "artifact": { "path": "gson/gson-2.10.1.jar" } } },
                { "name": "cpw.mods:bootstraplauncher:2.0.2",
                  "downloads": { "artifact": { "path": "bootstraplauncher-2.0.2.jar" } } }
            ]
        });

        let merged = merge_version_jsons(parent, child);
        let libs = merged["libraries"].as_array().unwrap_or(&vec![]).clone();

        assert_eq!(
            libs.len(),
            4,
            "Debe haber 4 libraries únicas, hay: {}. gson duplicado no fue eliminado.",
            libs.len()
        );

        let gson_count = libs
            .iter()
            .filter(|lib| {
                lib.get("name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.contains("com.google.code.gson:gson:"))
                    .unwrap_or(false)
            })
            .count();

        assert_eq!(
            gson_count, 1,
            "gson debe aparecer exactamente 1 vez, aparece: {}",
            gson_count
        );

        let has_bootstrap = libs.iter().any(|lib| {
            lib.get("name")
                .and_then(|v| v.as_str())
                .map(|s| s.contains("bootstraplauncher"))
                .unwrap_or(false)
        });
        assert!(
            has_bootstrap,
            "bootstraplauncher de child debe estar presente"
        );
    }

    #[test]
    fn verify_classpath_detects_duplicates() {
        let mut logs = Vec::new();
        let classpath_entries = vec![
            "/libs/gson-2.10.1.jar".to_string(),
            "/libs/authlib-6.0.54.jar".to_string(),
            "/libs/gson-2.10.1.jar".to_string(),
            "/libs/slf4j-api-2.0.9.jar".to_string(),
        ];

        let result = verify_no_duplicate_classpath_entries(&classpath_entries, &mut logs);
        assert!(result.is_err(), "debe fallar cuando hay duplicados");
        let message = result.err().unwrap_or_default();
        assert!(
            message.contains("Duplicate key"),
            "el error debe mencionar Duplicate key"
        );
    }

    #[test]
    fn maven_key_distinguishes_classifier() {
        let principal = json!({ "name": "org.lwjgl:lwjgl:3.3.3" });
        let natives = json!({ "name": "org.lwjgl:lwjgl:3.3.3:natives-windows" });
        let natives_arm = json!({ "name": "org.lwjgl:lwjgl:3.3.3:natives-windows-arm64" });

        let key_principal = extract_maven_key(&principal).unwrap_or_default();
        let key_natives = extract_maven_key(&natives).unwrap_or_default();
        let key_natives_arm = extract_maven_key(&natives_arm).unwrap_or_default();

        assert_ne!(key_principal, key_natives);
        assert_ne!(key_principal, key_natives_arm);
        assert_ne!(key_natives, key_natives_arm);

        assert_eq!(key_principal, "org.lwjgl:lwjgl");
        assert_eq!(key_natives, "org.lwjgl:lwjgl:natives-windows");
        assert_eq!(key_natives_arm, "org.lwjgl:lwjgl:natives-windows-arm64");
    }

    #[test]
    fn natives_windows_arm64_not_extracted_on_x86_64() {
        if cfg!(target_os = "windows") && std::env::consts::ARCH == "x86_64" {
            assert!(should_extract_for_platform(
                "lwjgl-3.3.3-natives-windows.jar"
            ));
            assert!(!should_extract_for_platform(
                "lwjgl-3.3.3-natives-windows-arm64.jar"
            ));
            assert!(!should_extract_for_platform(
                "lwjgl-3.3.3-natives-windows-x86.jar"
            ));
            assert!(!should_extract_for_platform(
                "lwjgl-3.3.3-natives-linux.jar"
            ));
            assert!(!should_extract_for_platform(
                "lwjgl-3.3.3-natives-macos.jar"
            ));
        }
    }

    #[test]
    fn dedup_preserves_both_principal_and_natives() {
        let libs = vec![
            json!({ "name": "org.lwjgl:lwjgl:3.3.3" }),
            json!({ "name": "org.lwjgl:lwjgl:3.3.3:natives-windows" }),
            json!({ "name": "org.lwjgl:lwjgl:3.3.3:natives-windows-arm64" }),
            json!({ "name": "com.google.code.gson:gson:2.10.1" }),
            json!({ "name": "com.google.code.gson:gson:2.10.1" }),
        ];

        let mut seen = std::collections::HashMap::new();
        let mut fallback_idx = 0usize;
        for lib in &libs {
            let key = extract_maven_key(lib).unwrap_or_else(|| {
                let key = format!("unknown_{fallback_idx}");
                fallback_idx += 1;
                key
            });
            seen.entry(key).or_insert_with(|| lib.clone());
        }

        assert_eq!(seen.len(), 4);
    }
}
