use std::{fs, path::Path};

use crate::{
    domain::models::instance::InstanceMetadata,
    infrastructure::filesystem::file_ops::write_placeholder_file, shared::result::AppResult,
};

pub fn build_instance_structure(
    instance_root: &Path,
    minecraft_root: &Path,
    minecraft_version: &str,
    logs: &mut Vec<String>,
) -> AppResult<()> {
    let structure_dirs = [
        instance_root.join("logs"),
        instance_root.join("mods"),
        instance_root.join("config"),
        instance_root.join("resourcepacks"),
        instance_root.join("shaderpacks"),
        instance_root.join("saves"),
        minecraft_root.join("assets"),
        minecraft_root.join("libraries"),
        minecraft_root.join("versions").join(minecraft_version),
        minecraft_root.join("mods"),
        minecraft_root.join("config"),
        minecraft_root.join("logs"),
        minecraft_root.join("crash-reports"),
        minecraft_root.join("saves"),
    ];

    for dir in structure_dirs {
        fs::create_dir_all(&dir)
            .map_err(|err| format!("No se pudo crear el directorio {}: {err}", dir.display()))?;
    }
    logs.push("Estructura interna de instancia y .minecraft creada.".to_string());

    let version_file_base = minecraft_root.join("versions").join(minecraft_version);
    let jar_path = version_file_base.join(format!("{minecraft_version}.jar"));
    let json_path = version_file_base.join(format!("{minecraft_version}.json"));

    write_placeholder_file(&jar_path, "placeholder minecraft jar")?;
    write_placeholder_file(
        &json_path,
        &format!("{{\"id\":\"{}\",\"type\":\"release\"}}", minecraft_version),
    )?;

    logs.push(
        "Classpath generado (placeholder para implementación del launcher runtime).".to_string(),
    );
    Ok(())
}

pub fn persist_instance_metadata(
    instance_root: &Path,
    metadata: &InstanceMetadata,
    logs: &mut Vec<String>,
) -> AppResult<()> {
    let metadata_path = instance_root.join(".instance.json");
    let metadata_content = serde_json::to_string_pretty(metadata).map_err(|err| err.to_string())?;
    fs::write(&metadata_path, metadata_content).map_err(|err| {
        format!(
            "No se pudo guardar la metadata de la instancia en {}: {err}",
            metadata_path.display()
        )
    })?;
    logs.push(format!(
        "Metadata guardada en {} (java: {}).",
        metadata_path.display(),
        metadata.java_path
    ));
    Ok(())
}
