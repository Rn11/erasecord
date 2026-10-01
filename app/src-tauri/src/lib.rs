mod commands;
mod state;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init());
    // Updates are checked against the signed latest.json of the newest
    // GitHub release; see plugins.updater in tauri.conf.json.
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    builder
        .manage(state::AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::login,
            commands::restore_session,
            commands::logout,
            commands::list_targets,
            commands::list_channels,
            commands::list_friends,
            commands::open_dm,
            commands::start_scan,
            commands::stop_scan,
            commands::start_job,
            commands::start_package_job,
            commands::export_run,
            commands::save_png,
            commands::unfinished_run,
            commands::resume_run,
            commands::discard_run,
            commands::import_package,
            commands::close_package,
            commands::preview_package,
            commands::insights_info,
            commands::generate_passphrase,
            commands::open_backup,
            commands::insights_overview,
            commands::insights_time,
            commands::insights_places,
            commands::insights_words,
            commands::insights_links,
            commands::insights_search,
            commands::pause_job,
            commands::resume_job,
            commands::cancel_job,
        ])
        .run(tauri::generate_context!())
        .expect("error while running EraseCord");
}
