use tauri_plugin_autostart::MacosLauncher;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            use tauri_plugin_autostart::ManagerExt;
            let autostart_manager = app.autolaunch();
            let _ = autostart_manager.enable();
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
