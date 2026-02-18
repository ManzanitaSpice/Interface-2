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
            app::launcher_service::list_instances,
            app::instance_service::open_instance_folder,
            app::instance_service::get_instance_metadata,
            app::instance_service::validate_and_prepare_launch,
            app::instance_service::start_instance
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
