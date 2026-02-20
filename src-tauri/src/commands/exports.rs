use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::{Path, PathBuf}};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub output_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceExportRequest {
    pub instance_root: String,
    pub instance_name: String,
    pub export_format: String,
}

fn slugify(name: &str) -> String {
    let cleaned = name
        .trim()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' { ch } else { '-' })
        .collect::<String>();
    cleaned.trim_matches('-').to_string()
}

fn add_dir_recursively(
    zip: &mut ZipWriter<std::fs::File>,
    base: &Path,
    current: &Path,
    options: SimpleFileOptions,
) -> Result<(), String> {
    let entries = fs::read_dir(current).map_err(|err| format!("No se pudo leer directorio {}: {err}", current.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let relative = path
            .strip_prefix(base)
            .map_err(|err| format!("Ruta relativa inválida: {err}"))?
            .to_string_lossy()
            .replace('\\', "/");

        if path.is_dir() {
            let dir_name = format!("{relative}/");
            zip.add_directory(dir_name, options)
                .map_err(|err| format!("No se pudo agregar carpeta al zip: {err}"))?;
            add_dir_recursively(zip, base, &path, options)?;
            continue;
        }

        let bytes = fs::read(&path).map_err(|err| format!("No se pudo leer archivo {}: {err}", path.display()))?;
        zip.start_file(relative, options)
            .map_err(|err| format!("No se pudo agregar archivo al zip: {err}"))?;
        zip.write_all(&bytes)
            .map_err(|err| format!("No se pudo escribir archivo en zip: {err}"))?;
    }

    Ok(())
}

#[tauri::command]
pub fn export_instance_package(request: InstanceExportRequest) -> Result<ExportResult, String> {
    let instance_root = PathBuf::from(&request.instance_root);
    if !instance_root.exists() {
        return Err("La instancia no existe en disco".into());
    }

    let extension = if request.export_format == "mrpack" { "mrpack" } else { "zip" };
    let suggested = format!("{}-{}.{}", slugify(&request.instance_name), request.export_format.to_lowercase(), extension);

    let file = rfd::FileDialog::new()
        .set_title("Exportar instancia")
        .set_file_name(&suggested)
        .save_file();

    let Some(output_path) = file else {
        return Err("Exportación cancelada por el usuario".into());
    };

    let output_file = std::fs::File::create(&output_path)
        .map_err(|err| format!("No se pudo crear archivo de exportación: {err}"))?;

    let mut zip = ZipWriter::new(output_file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let export_manifest = serde_json::json!({
        "name": request.instance_name,
        "format": request.export_format,
        "exportedBy": "Interface Launcher",
        "version": 1,
    });

    zip.start_file("interface-export.json", options)
        .map_err(|err| format!("No se pudo iniciar manifest de exportación: {err}"))?;
    zip.write_all(export_manifest.to_string().as_bytes())
        .map_err(|err| format!("No se pudo escribir manifest: {err}"))?;

    add_dir_recursively(&mut zip, &instance_root, &instance_root, options)?;

    zip.finish()
        .map_err(|err| format!("No se pudo finalizar el archivo: {err}"))?;

    Ok(ExportResult {
        output_path: output_path.display().to_string(),
    })
}
