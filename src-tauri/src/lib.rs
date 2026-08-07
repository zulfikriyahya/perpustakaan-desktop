use tauri::Manager;
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_shell::ShellExt;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        if shortcut.matches(
                            tauri_plugin_global_shortcut::Modifiers::CONTROL
                                | tauri_plugin_global_shortcut::Modifiers::SHIFT,
                            tauri_plugin_global_shortcut::Code::KeyQ,
                        ) {
                            app.exit(0);
                        }
                    }
                })
                .build(),
        )
        .setup(|app| {
            let autostart_manager = app.autolaunch();
            let _ = autostart_manager.enable();

            app.global_shortcut().register("Ctrl+Shift+Q")?;

            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_fullscreen(true);
            }

            let sidecar = app.shell().sidecar("rfid_bridge")
                .expect("gagal siapkan sidecar rfid_bridge");
            let (mut _rx, _child) = sidecar
                .spawn()
                .expect("gagal menjalankan rfid_bridge");

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
