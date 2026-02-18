use std::{path::PathBuf, process::Command};

use super::java_version::JavaRuntime;

#[derive(Debug, Clone)]
pub struct JavaCandidate {
    pub path: PathBuf,
    pub major: u32,
}

pub fn find_compatible_java(required: JavaRuntime) -> Option<JavaCandidate> {
    detect_java_from_path().filter(|candidate| candidate.major >= u32::from(required.major()))
}

fn detect_java_from_path() -> Option<JavaCandidate> {
    let output = Command::new("java").arg("-version").output().ok()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stderr}\n{stdout}");
    let major = parse_java_major(&combined)?;
    let path = resolve_java_path_from_path_env().unwrap_or_else(|| PathBuf::from("java"));

    Some(JavaCandidate { path, major })
}

fn resolve_java_path_from_path_env() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        let output = Command::new("where").arg("java").output().ok()?;
        let body = String::from_utf8_lossy(&output.stdout);
        body.lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(PathBuf::from)
    } else {
        let output = Command::new("which").arg("java").output().ok()?;
        let body = String::from_utf8_lossy(&output.stdout);
        body.lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(PathBuf::from)
    }
}

fn parse_java_major(version_output: &str) -> Option<u32> {
    let quoted = version_output.split('"').nth(1)?;

    if let Some(rest) = quoted.strip_prefix("1.") {
        return rest.split('.').next()?.parse::<u32>().ok();
    }

    quoted.split('.').next()?.parse::<u32>().ok()
}

#[cfg(test)]
mod tests {
    use super::parse_java_major;

    #[test]
    fn parses_legacy_java_version() {
        let sample = "java version \"1.8.0_372\"";
        assert_eq!(parse_java_major(sample), Some(8));
    }

    #[test]
    fn parses_modern_java_version() {
        let sample = "openjdk version \"21.0.4\" 2024-07-16";
        assert_eq!(parse_java_major(sample), Some(21));
    }
}
