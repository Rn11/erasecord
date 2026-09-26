mod commands;
mod state;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(state::AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::login,
            commands::restore_session,
            commands::logout,
            commands::list_targets,
            commands::list_channels,
            commands::list_friends,
            commands::open_dm,
            commands::preview,
            commands::start_job,
            commands::pause_job,
            commands::resume_job,
            commands::cancel_job,
        ])
        .run(tauri::generate_context!())
        .expect("error while running purgecord");
}
