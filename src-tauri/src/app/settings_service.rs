use std::path::PathBuf;

#[derive(serde::Serialize)]
pub struct PickedFolderResult {
    pub path: Option<String>,
}

#[tauri::command]
pub fn pick_folder(initial_path: Option<String>, title: Option<String>) -> Result<PickedFolderResult, String> {
    let mut dialog = rfd::FileDialog::new();

    if let Some(path) = initial_path {
        let sanitized = path.trim();
        if !sanitized.is_empty() {
            dialog = dialog.set_directory(PathBuf::from(sanitized));
        }
    }

    if let Some(custom_title) = title {
        let sanitized = custom_title.trim();
        if !sanitized.is_empty() {
            dialog = dialog.set_title(sanitized);
        }
    }

    let selected = dialog.pick_folder();
    Ok(PickedFolderResult {
        path: selected.map(|folder| folder.to_string_lossy().to_string()),
    })
}
