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

// TODO: ASUMSI - "reachable"/"tidak error" didefinisikan sebagai status 2xx
// saja (sebelumnya: apa saja selain 5xx). Ini artinya 4xx (404/403/dst)
// sekarang juga dianggap error dan memicu splash+reload otomatis. Kalau web
// app sirkulasi punya endpoint yang sengaja balas 4xx dalam kondisi normal,
// definisi ini perlu direvisi.
fn is_reachable_status(status: reqwest::StatusCode) -> bool {
    status.is_success()
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

                    // F5 - TODO: ASUMSI - shortcut tambahan (bukan pengganti Ctrl+Shift+R)
                    // untuk trigger reload yang sama.
                    if shortcut.matches(
                        tauri_plugin_global_shortcut::Modifiers::empty(),
                        tauri_plugin_global_shortcut::Code::F5,
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
        .invoke_handler(tauri::generate_handler![
    check_url_reachable,
    request_focus,
    send_activation_click
])
        .setup(|app| {
            let autostart_manager = app.autolaunch();
            let _ = autostart_manager.enable();

            app.global_shortcut().register("Ctrl+Shift+W")?;
            app.global_shortcut().register("Ctrl+Shift+R")?;
            app.global_shortcut().register("F5")?;
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
            let focus_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                for delay_secs in [1, 3, 6] {
                    tokio::time::sleep(Duration::from_secs(delay_secs)).await;
                    if let Some(window) = focus_handle.get_webview_window("main") {
                        let _ = window.set_focus();
                    }
                }
            });
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

#[tauri::command]
async fn check_url_reachable(url: String) -> Result<bool, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(HEALTH_CHECK_TIMEOUT_SECS))
        .build()
        .map_err(|e| e.to_string())?;

    match client.get(&url).send().await {
        Ok(resp) => Ok(is_reachable_status(resp.status())),
        Err(_) => Ok(false),
    }
}

#[tauri::command]
fn request_focus(app: tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_focus();
    }
}

// Kirim perintah "CLICK x y" ke sidecar rfid_bridge lewat stdin - sidecar
// yang akan mengeksekusi klik mouse sintetis via enigo. Dipakai untuk
// memberi "aktivasi pengguna" asli agar browser mengizinkan fokus ke form
// input di iframe cross-origin (autofocus JS biasa diblokir browser untuk
// kasus ini).
//
// TODO: verifikasi signature CommandChild::write terhadap versi
// tauri-plugin-shell 2.3.5 yang terpasang - method dan error type di sini
// diasumsikan dari dokumentasi umum, belum diverifikasi lewat build aktual.
#[tauri::command]
fn send_activation_click(app: tauri::AppHandle, x: i32, y: i32) -> Result<(), String> {
    let state = app.state::<RfidBridgeState>();
    let mut guard = state.0.lock().unwrap();
    if let Some(child) = guard.as_mut() {
        let cmd = format!("CLICK {} {}\n", x, y);
        child.write(cmd.as_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

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
                continue;
            };

            let reachable = match client.get(&url).send().await {
                Ok(resp) => is_reachable_status(resp.status()),
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
