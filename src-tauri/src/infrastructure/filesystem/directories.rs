use std::{fs, path::Path};

use crate::shared::result::AppResult;

pub fn create_launcher_directories(root: &Path, logs: &mut Vec<String>) -> AppResult<()> {
    let dirs = [
        root.join("runtime/java8"),
        root.join("runtime/java17"),
        root.join("runtime/java21"),
        root.join("instances"),
        root.join("assets"),
        root.join("cache"),
        root.join("logs"),
        root.join("config"),
        root.join("versions"),
    ];

    for dir in dirs {
        fs::create_dir_all(&dir)
            .map_err(|err| format!("No se pudo crear el directorio {}: {err}", dir.display()))?;
    }

    let launcher_config = root.join("config/launcher.json");
    if !launcher_config.exists() {
        fs::write(
            &launcher_config,
            "{\n  \"defaultPage\": \"Mis Modpacks\",\n  \"javaPath\": \"runtime/java17/bin/java\"\n}\n",
        )
        .map_err(|err| {
            format!(
                "No se pudo crear el archivo de configuración {}: {err}",
                launcher_config.display()
            )
        })?;
    }

    let accounts_config = root.join("config/accounts.json");
    if !accounts_config.exists() {
        fs::write(&accounts_config, "[]\n").map_err(|err| {
            format!(
                "No se pudo crear el archivo de cuentas {}: {err}",
                accounts_config.display()
            )
        })?;
    }

    logs.push("Estructura global del launcher verificada/creada.".to_string());
    Ok(())
}
