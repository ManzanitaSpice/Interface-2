use std::{fs, path::Path};

use crate::shared::result::AppResult;

pub fn write_placeholder_file(path: &Path, content: &str) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "No se pudo crear el directorio padre {}: {err}",
                parent.display()
            )
        })?;
    }
    fs::write(path, content).map_err(|err| {
        format!(
            "No se pudo escribir el archivo placeholder {}: {err}",
            path.display()
        )
    })?;
    Ok(())
}
