pub mod app;
pub mod commands;
pub mod domain;
pub mod infrastructure;
pub mod platform;
pub mod runtime;
pub mod services;
pub mod shared;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            app::launcher_service::create_instance,
            app::launcher_service::list_instances,
            app::launcher_service::delete_instance,
            app::auth_service::list_available_browsers,
            app::auth_service::open_url_in_browser,
            app::auth_service::authorize_microsoft_in_launcher,
            app::auth_service::start_microsoft_auth,
            app::auth_service::complete_microsoft_auth,
            app::auth_service::refresh_microsoft_auth,
            app::auth_service::start_microsoft_device_auth,
            app::auth_service::complete_microsoft_device_auth,
            app::instance_service::open_instance_folder,
            app::instance_service::open_redirect_origin_folder,
            app::instance_service::get_instance_metadata,
            app::instance_service::get_instance_card_stats,
            app::instance_service::validate_and_prepare_launch,
            app::instance_service::start_instance,
            app::instance_service::get_runtime_status,
            app::instance_service::force_close_instance,
            app::redirect_launch::validate_redirect_instance,
            app::redirect_launch::get_redirect_cache_info,
            app::redirect_launch::force_cleanup_redirect_cache,
            app::redirect_launch::repair_instance,
            app::redirect_launch::repair_all_instances,
            app::settings_service::pick_folder,
            app::settings_service::load_folder_routes,
            app::settings_service::save_folder_routes,
            app::settings_service::open_folder_path,
            app::settings_service::open_folder_route,
            app::settings_service::migrate_instances_folder,
            commands::settings::get_launcher_folders,
            commands::settings::migrate_launcher_root,
            commands::settings::change_instances_folder,
            commands::settings::get_instances_count,
            commands::import::detect_external_instances,
            commands::import::import_specific,
            commands::import::execute_import,
            commands::import::execute_import_action,
            commands::import::execute_import_action_batch,
            commands::import::cancel_import,
            commands::catalog::search_catalogs,
            commands::catalog::get_catalog_detail,
            commands::mods::list_instance_mods,
            commands::mods::set_instance_mod_enabled,
            commands::mods::replace_instance_mod_file,
            commands::mods::install_catalog_mod_file,
            commands::exports::export_instance_package,
            commands::skin_processor::optimize_skin_png,
            commands::file_manager::list_skins,
            commands::file_manager::import_skin,
            commands::file_manager::delete_skin,
            commands::file_manager::load_skin_binary,
            commands::file_manager::save_skin_binary,
            commands::visual_meta::save_instance_visual_meta,
            commands::visual_meta::save_instance_visual_media,
            commands::visual_meta::load_instance_visual_meta
        ])
        .setup(|app| {
            let _ = app::redirect_launch::cleanup_redirect_cache_on_startup(app.handle());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
