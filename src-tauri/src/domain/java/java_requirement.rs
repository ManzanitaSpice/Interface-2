use std::cmp::max;

use super::java_version::JavaRuntime;

pub fn determine_required_java(mc_version: &str, loader: &str) -> Result<JavaRuntime, String> {
    let mc_req = java_for_minecraft(mc_version)?;
    let loader_req = java_for_loader(loader, mc_version)?;
    Ok(max(mc_req, loader_req))
}

fn java_for_loader(loader: &str, mc_version: &str) -> Result<JavaRuntime, String> {
    let loader_lower = loader.trim().to_ascii_lowercase();
    if [
        "vanilla", "forge", "neoforge", "fabric", "quilt", "quilit", "snapshot",
    ]
    .contains(&loader_lower.as_str())
    {
        return java_for_minecraft(mc_version);
    }

    Err(format!("Loader no soportado: {loader}"))
}

fn java_for_minecraft(mc_version: &str) -> Result<JavaRuntime, String> {
    let (major, minor, patch) = parse_mc_version(mc_version)?;

    if major != 1 {
        return Err(format!(
            "Versión de Minecraft no soportada para auto-detección de Java: {mc_version}"
        ));
    }

    if minor <= 16 {
        return Ok(JavaRuntime::Java8);
    }

    if minor <= 20 {
        let p = patch.unwrap_or(0);
        if minor == 20 && p >= 5 {
            return Ok(JavaRuntime::Java21);
        }
        return Ok(JavaRuntime::Java17);
    }

    Ok(JavaRuntime::Java21)
}

pub fn parse_mc_version(version: &str) -> Result<(u32, u32, Option<u32>), String> {
    let core = version
        .split(['-', ' '])
        .next()
        .ok_or_else(|| format!("Versión inválida: {version}"))?;

    let mut parts = core.split('.');
    let major = parts
        .next()
        .ok_or_else(|| format!("Versión inválida: {version}"))?
        .parse::<u32>()
        .map_err(|_| format!("No se pudo parsear versión de Minecraft: {version}"))?;
    let minor = parts
        .next()
        .ok_or_else(|| format!("Versión inválida: {version}"))?
        .parse::<u32>()
        .map_err(|_| format!("No se pudo parsear versión de Minecraft: {version}"))?;
    let patch = parts.next().and_then(|value| value.parse::<u32>().ok());

    Ok((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::{determine_required_java, parse_mc_version};
    use crate::domain::java::java_version::JavaRuntime;

    #[test]
    fn mc_version_ranges_map_to_java_versions() {
        assert_eq!(
            determine_required_java("1.16.5", "vanilla").unwrap(),
            JavaRuntime::Java8
        );
        assert_eq!(
            determine_required_java("1.20.4", "fabric").unwrap(),
            JavaRuntime::Java17
        );
        assert_eq!(
            determine_required_java("1.20.5", "quilt").unwrap(),
            JavaRuntime::Java21
        );
        assert_eq!(
            determine_required_java("1.21.4", "neoforge").unwrap(),
            JavaRuntime::Java21
        );
    }

    #[test]
    fn parser_handles_suffixes() {
        assert_eq!(
            parse_mc_version("1.20.1-forge-47.3.0").unwrap(),
            (1, 20, Some(1))
        );
        assert_eq!(parse_mc_version("1.21.4").unwrap(), (1, 21, Some(4)));
    }
}
