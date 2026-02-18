pub mod app;
pub mod domain;
pub mod infrastructure;
pub mod platform;
pub mod runtime;
pub mod services;
pub mod shared;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            app::launcher_service::create_instance,
            app::instance_service::open_instance_folder
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
