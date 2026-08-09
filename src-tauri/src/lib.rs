use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use serde::Serialize;
use tauri::{Emitter, Manager, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_shell::{process::CommandChild, ShellExt};
use tauri_plugin_store::StoreExt;

struct RfidBridgeState(Mutex<Option<CommandChild>>);

const CONFIG_STORE_FILE: &str = "app_config.json";
const URL_STORE_KEY: &str = "circulation_url";

// TODO: ASUMSI - interval, timeout, dan grace period berikut adalah default yang wajar,
// bukan diminta eksplisit. Sesuaikan bila perlu.
const HEALTH_CHECK_INTERVAL_SECS: u64 = 15;
const HEALTH_CHECK_TIMEOUT_SECS: u64 = 5;
const POST_RELOAD_GRACE_SECS: u64 = 30;

#[derive(Clone, Serialize)]
struct CirculationStatus {
    reachable: bool,
    url: String,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }

                    // Ctrl+Shift+W - keluar aplikasi
                    if shortcut.matches(
                        tauri_plugin_global_shortcut::Modifiers::CONTROL
                            | tauri_plugin_global_shortcut::Modifiers::SHIFT,
                        tauri_plugin_global_shortcut::Code::KeyW,
                    ) {
                        kill_sidecar(app);
                        app.exit(0);
                        return;
                    }

                    // Ctrl+Shift+R - reload konten (iframe) + re-check koneksi
                    if shortcut.matches(
                        tauri_plugin_global_shortcut::Modifiers::CONTROL
                            | tauri_plugin_global_shortcut::Modifiers::SHIFT,
                        tauri_plugin_global_shortcut::Code::KeyR,
                    ) {
                        let _ = app.emit_to("main", "force-reload", ());
                        return;
                    }

                    // Ctrl+Shift+U - buka form ganti URL sirkulasi
                    if shortcut.matches(
                        tauri_plugin_global_shortcut::Modifiers::CONTROL
                            | tauri_plugin_global_shortcut::Modifiers::SHIFT,
                        tauri_plugin_global_shortcut::Code::KeyU,
                    ) {
                        let _ = app.emit_to("main", "open-url-config", ());
                    }
                })
                .build(),
        )
        .manage(RfidBridgeState(Mutex::new(None)))
        .invoke_handler(tauri::generate_handler![check_url_reachable])
        .setup(|app| {
            let autostart_manager = app.autolaunch();
            let _ = autostart_manager.enable();

            app.global_shortcut().register("Ctrl+Shift+W")?;
            app.global_shortcut().register("Ctrl+Shift+R")?;
            app.global_shortcut().register("Ctrl+Shift+U")?;

            if let Some(window) = app.get_webview_window("main") {
                let win = window.clone();
                let did_fullscreen = Arc::new(AtomicBool::new(false));
                window.on_window_event(move |event| {
                    if let WindowEvent::Focused(true) = event {
                        if !did_fullscreen.swap(true, Ordering::SeqCst) {
                            if let Err(e) = win.set_fullscreen(true) {
                                eprintln!("gagal set fullscreen: {e}");
                                let _ = win.maximize();
                            }
                        }
                    }
                });
            }

            let sidecar = app
                .shell()
                .sidecar("rfid_bridge")
                .expect("gagal siapkan sidecar rfid_bridge");
            let (mut _rx, child) = sidecar.spawn().expect("gagal menjalankan rfid_bridge");

            let state = app.state::<RfidBridgeState>();
            *state.0.lock().unwrap() = Some(child);

            spawn_health_check_monitor(app.handle().clone());

            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed => {
                kill_sidecar(window.app_handle());
            }
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn kill_sidecar(app: &tauri::AppHandle) {
    let state = app.state::<RfidBridgeState>();
    let mut guard = state.0.lock().unwrap();
    if let Some(child) = guard.take() {
        let _ = child.kill();
    }
}

// Command dipanggil dari frontend (JS) untuk cek apakah sebuah URL reachable
// sebelum ditampilkan di iframe. Dipakai juga saat operator menyimpan URL baru.
#[tauri::command]
async fn check_url_reachable(url: String) -> Result<bool, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(HEALTH_CHECK_TIMEOUT_SECS))
        .build()
        .map_err(|e| e.to_string())?;

    match client.get(&url).send().await {
        Ok(resp) => Ok(!resp.status().is_server_error()),
        Err(_) => Ok(false),
    }
}

// Polling periodik membaca URL tersimpan dari store, cek reachability, dan
// mengirim event "circulation-status" ke frontend. Frontend yang menentukan
// tampilan (splash/config vs iframe) berdasarkan event ini - Rust tidak lagi
// memanggil reload/eval window secara langsung.
fn spawn_health_check_monitor(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(HEALTH_CHECK_TIMEOUT_SECS))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                eprintln!("gagal inisialisasi HTTP client untuk health-check: {e}");
                return;
            }
        };

        let mut last_reachable: Option<bool> = None;

        loop {
            tokio::time::sleep(Duration::from_secs(HEALTH_CHECK_INTERVAL_SECS)).await;

            let url = match app.store(CONFIG_STORE_FILE) {
                Ok(store) => store
                    .get(URL_STORE_KEY)
                    .and_then(|v| v.as_str().map(|s| s.to_string())),
                Err(e) => {
                    eprintln!("gagal buka store konfigurasi: {e}");
                    None
                }
            };

            let Some(url) = url else {
                // Belum ada URL tersimpan - tidak ada yang dimonitor.
                continue;
            };

            let reachable = match client.get(&url).send().await {
                Ok(resp) => !resp.status().is_server_error(),
                Err(_) => false,
            };

            let became_unreachable = last_reachable == Some(true) && !reachable;
            let became_reachable = last_reachable != Some(true) && reachable;

            if became_unreachable {
                eprintln!("koneksi ke {} terputus/bermasalah", url);
            }
            if became_reachable {
                eprintln!("koneksi ke {} pulih", url);
            }

            // Kirim status tiap siklus supaya frontend selalu sinkron, meski
            // tidak berubah - biaya kecil, menyederhanakan state di JS.
            let _ = app.emit_to(
                "main",
                "circulation-status",
                CirculationStatus {
                    reachable,
                    url: url.clone(),
                },
            );

            last_reachable = Some(reachable);

            if !reachable {
                tokio::time::sleep(Duration::from_secs(POST_RELOAD_GRACE_SECS)).await;
            }
        }
    });
}
