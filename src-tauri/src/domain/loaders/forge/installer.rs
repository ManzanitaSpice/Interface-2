use std::path::Path;
use std::process::Command;

use crate::shared::result::AppResult;

pub const MIN_JAVA_VERSION_MODERN_FORGE: u32 = 17;

pub fn modern_installer_args(minecraft_version: &str, with_debug: bool) -> Vec<String> {
    let mut args = vec![
        "--installClient".to_string(),
        "--mcversion".to_string(),
        minecraft_version.to_string(),
    ];
    if with_debug {
        args.push("--debug".to_string());
    }
    args
}

pub fn ensure_modern_forge_java(java_exec: &Path, loader_name: &str) -> AppResult<u32> {
    let output = Command::new(java_exec)
        .arg("-version")
        .output()
        .map_err(|err| format!("No se pudo ejecutar java -version para {loader_name}: {err}"))?;

    let raw = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let major = parse_java_major_version(&raw).ok_or_else(|| {
        format!(
            "No se pudo detectar versión de Java para {loader_name}. Salida: {}",
            raw.trim()
        )
    })?;

    if major < MIN_JAVA_VERSION_MODERN_FORGE {
        return Err(format!(
            "{loader_name} requiere Java {}+ y se detectó Java {major}.",
            MIN_JAVA_VERSION_MODERN_FORGE
        ));
    }

    Ok(major)
}

fn parse_java_major_version(raw: &str) -> Option<u32> {
    let token = raw
        .split_whitespace()
        .find(|part| part.starts_with('"') && part.ends_with('"'))?
        .trim_matches('"');

    if let Some(rest) = token.strip_prefix("1.") {
        return rest.split('.').next()?.parse::<u32>().ok();
    }

    token.split('.').next()?.parse::<u32>().ok()
}

#[cfg(test)]
mod tests {
    use super::parse_java_major_version;

    #[test]
    fn parses_legacy_java_version() {
        assert_eq!(
            parse_java_major_version("java version \"1.8.0_392\""),
            Some(8)
        );
    }

    #[test]
    fn parses_modern_java_version() {
        assert_eq!(
            parse_java_major_version("openjdk version \"17.0.11\" 2024-04-16"),
            Some(17)
        );
    }
}
