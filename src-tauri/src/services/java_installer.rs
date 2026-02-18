use std::{ffi::OsStr, fs, io::Cursor, path::Path, path::PathBuf};

use flate2::read::GzDecoder;
use tar::Archive;
use zip::ZipArchive;

use crate::{
    domain::models::java::JavaRuntime,
    infrastructure::{
        checksum::sha1::sha256_hex,
        downloader::{
            client::{build_http_client, resolve_temurin_asset},
            integrity::validate_checksum,
        },
        filesystem::paths::java_executable_path,
    },
    shared::result::AppResult,
};

pub fn ensure_embedded_java(
    root: &Path,
    runtime: JavaRuntime,
    logs: &mut Vec<String>,
) -> AppResult<PathBuf> {
    let arch = crate::platform::windows::detect_architecture()?;
    logs.push(format!("Arquitectura detectada: {arch}."));

    let runtime_root = root.join("runtime").join(runtime.as_dir_name());
    let java_exec = java_executable_path(&runtime_root);
    if java_exec.exists() {
        logs.push(format!(
            "Java {} ya instalado: {}",
            runtime.major(),
            java_exec.display()
        ));
        return Ok(java_exec);
    }

    fs::create_dir_all(&runtime_root)?;
    logs.push(format!(
        "Java {} no encontrado. Iniciando descarga de runtime embebido oficial (Temurin).",
        runtime.major()
    ));

    let client = build_http_client()?;
    let (download_url, expected_checksum, file_name) = resolve_temurin_asset(&client, runtime)?;

    logs.push(format!("Descargando: {download_url}"));
    let archive_bytes = client
        .get(&download_url)
        .send()
        .and_then(|resp| resp.error_for_status())
        .map_err(|err| format!("Fallo la descarga del JDK: {err}"))?
        .bytes()
        .map_err(|err| format!("No se pudo leer el binario descargado: {err}"))?
        .to_vec();

    let archive_sha = sha256_hex(&archive_bytes);
    validate_checksum(&expected_checksum, &archive_sha, runtime.major())?;

    logs.push(format!(
        "Checksum SHA-256 validado para Java {}.",
        runtime.major()
    ));

    extract_archive(&archive_bytes, &file_name, &runtime_root)?;

    if !java_exec.exists() {
        return Err(format!(
            "Se extrajo el runtime de Java {}, pero no se encontró ejecutable en {}",
            runtime.major(),
            java_exec.display()
        ));
    }

    let marker = runtime_root.join(".installed.json");
    fs::write(
        &marker,
        serde_json::json!({
            "runtime": runtime.as_dir_name(),
            "javaMajor": runtime.major(),
            "downloadUrl": download_url,
            "checksum": expected_checksum,
            "archive": file_name,
            "status": "installed"
        })
        .to_string(),
    )?;

    logs.push(format!(
        "Java {} instalado y marcado como listo en {}.",
        runtime.major(),
        marker.display()
    ));

    Ok(java_exec)
}

fn extract_archive(archive: &[u8], file_name: &str, destination: &Path) -> AppResult<()> {
    let normalized = file_name.to_ascii_lowercase();

    if normalized.ends_with(".zip") {
        extract_zip_archive(archive, destination)
    } else if normalized.ends_with(".tar.gz") || normalized.ends_with(".tgz") {
        let decoder = GzDecoder::new(Cursor::new(archive));
        let mut tar = Archive::new(decoder);
        tar.unpack(destination)
            .map_err(|err| format!("No se pudo extraer tar.gz: {err}"))?;
        flatten_single_top_level_dir(destination)
    } else {
        Err(format!(
            "Formato de archivo no soportado para runtime Java: {file_name}"
        ))
    }
}

fn extract_zip_archive(archive: &[u8], destination: &Path) -> AppResult<()> {
    let reader = Cursor::new(archive);
    let mut zip = ZipArchive::new(reader).map_err(|err| format!("ZIP inválido: {err}"))?;

    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|err| format!("No se pudo leer entrada ZIP: {err}"))?;
        let out_path = match entry.enclosed_name() {
            Some(path) => destination.join(path),
            None => continue,
        };

        if entry.name().ends_with('/') {
            fs::create_dir_all(&out_path)
                .map_err(|err| format!("No se pudo crear carpeta al extraer ZIP: {err}"))?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                format!("No se pudo crear directorio padre al extraer ZIP: {err}")
            })?;
        }

        let mut file = fs::File::create(&out_path).map_err(|err| {
            format!(
                "No se pudo crear archivo extraído {}: {err}",
                out_path.display()
            )
        })?;
        std::io::copy(&mut entry, &mut file).map_err(|err| {
            format!(
                "No se pudo escribir archivo extraído {}: {err}",
                out_path.display()
            )
        })?;
    }

    flatten_single_top_level_dir(destination)
}

fn flatten_single_top_level_dir(destination: &Path) -> AppResult<()> {
    let mut entries = fs::read_dir(destination)
        .map_err(|err| {
            format!(
                "No se pudo leer el directorio de runtime {}: {err}",
                destination.display()
            )
        })?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();

    if entries.len() != 1 || !entries[0].path().is_dir() {
        return Ok(());
    }

    let top_dir = entries.remove(0).path();
    let children = fs::read_dir(&top_dir)
        .map_err(|err| {
            format!(
                "No se pudo leer carpeta interna {}: {err}",
                top_dir.display()
            )
        })?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();

    for child in children {
        let from = child.path();
        let name = from
            .file_name()
            .and_then(OsStr::to_str)
            .ok_or_else(|| "Ruta inválida al reorganizar runtime Java.".to_string())?;
        let to = destination.join(name);
        fs::rename(&from, &to).map_err(|err| {
            format!(
                "No se pudo mover {} a {} al reorganizar runtime: {err}",
                from.display(),
                to.display()
            )
        })?;
    }

    fs::remove_dir_all(&top_dir).map_err(|err| {
        format!(
            "No se pudo limpiar carpeta temporal {}: {err}",
            top_dir.display()
        )
    })?;
    Ok(())
}
