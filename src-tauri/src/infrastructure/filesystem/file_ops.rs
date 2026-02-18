use std::{fs, path::Path};

use crate::shared::result::AppResult;

pub fn write_placeholder_file(path: &Path, content: &str) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}
