use std::sync::Mutex;
use tauri::{Manager, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_shell::{process::CommandChild, ShellExt};

struct RfidBridgeState(Mutex<Option<CommandChild>>);

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
        .manage(RfidBridgeState(Mutex::new(None)))
        .setup(|app| {
            let autostart_manager = app.autolaunch();
            let _ = autostart_manager.enable();

            app.global_shortcut().register("Ctrl+Shift+Q")?;

            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_fullscreen(true);
            }

            let sidecar = app.shell().sidecar("rfid_bridge")
                .expect("gagal siapkan sidecar rfid_bridge");
            let (mut _rx, child) = sidecar
                .spawn()
                .expect("gagal menjalankan rfid_bridge");

            let state = app.state::<RfidBridgeState>();
            *state.0.lock().unwrap() = Some(child);

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { .. } = event {
                let state = window.state::<RfidBridgeState>();
                if let Some(child) = state.0.lock().unwrap().take() {
                    let _ = child.kill();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
