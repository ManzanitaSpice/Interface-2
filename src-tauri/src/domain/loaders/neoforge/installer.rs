use std::path::Path;

use crate::domain::loaders::forge::installer::{ensure_modern_forge_java, modern_installer_args};
use crate::shared::result::AppResult;

pub fn neoforge_installer_args(minecraft_version: &str) -> Vec<String> {
    modern_installer_args(minecraft_version, true)
}

pub fn ensure_neoforge_java(java_exec: &Path) -> AppResult<u32> {
    ensure_modern_forge_java(java_exec, "NeoForge")
}
