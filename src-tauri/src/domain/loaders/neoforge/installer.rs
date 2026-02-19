use std::path::Path;

use crate::domain::loaders::forge::installer::ensure_modern_forge_java;
use crate::shared::result::AppResult;

pub fn neoforge_installer_args() -> Vec<String> {
    vec!["--installClient".to_string()]
}

pub fn ensure_neoforge_java(java_exec: &Path) -> AppResult<u32> {
    ensure_modern_forge_java(java_exec, "NeoForge")
}
