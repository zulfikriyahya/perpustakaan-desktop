use serialport::{SerialPort, SerialPortType};
use std::io::{self, BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::thread;
use enigo::{Button, Coordinate, Direction, Enigo, Keyboard, Mouse, Settings};

fn find_esp32_port() -> Option<String> {
    let ports = serialport::available_ports().ok()?;
    for p in &ports {
        if let SerialPortType::UsbPort(info) = &p.port_type {
            let known = matches!(
                (info.vid, info.pid),
                (0x10C4, 0xEA60) | (0x1A86, 0x7523) | (0x1A86, 0x55D4)
            );
            if known {
                return Some(p.port_name.clone());
            }
        }
    }
    ports.into_iter().find_map(|p| {
        let name = &p.port_name;
        let looks_like_serial = name.starts_with("COM")
            || name.contains("ttyUSB")
            || name.contains("ttyACM")
            || name.contains("cu.usbserial")
            || name.contains("cu.usbmodem")
            || name.contains("cu.wchusbserial");
        if looks_like_serial {
            Some(name.clone())
        } else {
            None
        }
    })
}

fn connect_loop(verbose: bool) -> Box<dyn SerialPort> {
    loop {
        if let Some(port_name) = find_esp32_port() {
            match serialport::new(&port_name, 115200)
                .timeout(Duration::from_millis(50))
                .open()
            {
                Ok(port) => {
                    if verbose {
                        println!("Terhubung ke {}", port_name);
                    }
                    return port;
                }
                Err(e) => {
                    if verbose {
                        eprintln!("Ditemukan {} tapi gagal buka: {}. Coba lagi...", port_name, e);
                    }
                }
            }
        } else if verbose {
            println!("Menunggu perangkat RFID ditancapkan...");
        }
        thread::sleep(Duration::from_millis(1000));
    }
}

// Thread terpisah: baca perintah dari stdin (dikirim src-tauri lewat
// CommandChild::write) untuk memicu klik mouse sintetis. Dipakai supaya
// browser menganggap fokus ke form input sebagai aktivasi asli dari
// pengguna, bukan panggilan JavaScript (yang diblokir untuk autofocus
// cross-origin iframe). Enigo instance dibagi lewat Mutex dengan loop RFID
// utama supaya tidak ada dua kontrol input yang berebut.
//
// Format perintah (satu baris, dipisah newline): "CLICK <x> <y>"
fn spawn_stdin_command_listener(enigo: Arc<Mutex<Enigo>>, verbose: bool) {
    thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let mut parts = line.split_whitespace();
            match parts.next() {
                Some("CLICK") => {
                    let coords = (
                        parts.next().and_then(|v| v.parse::<i32>().ok()),
                        parts.next().and_then(|v| v.parse::<i32>().ok()),
                    );
                    if let (Some(x), Some(y)) = coords {
                        let mut guard = match enigo.lock() {
                            Ok(g) => g,
                            Err(e) => {
                                if verbose {
                                    eprintln!("gagal lock enigo untuk CLICK: {e}");
                                }
                                continue;
                            }
                        };
                        let _ = guard.move_mouse(x, y, Coordinate::Abs);
                        let _ = guard.button(Button::Left, Direction::Click);
                        if verbose {
                            println!("CLICK dieksekusi di ({x}, {y})");
                        }
                    } else if verbose {
                        eprintln!("perintah CLICK tidak valid: {line}");
                    }
                }
                _ => {
                    if verbose {
                        eprintln!("perintah stdin tidak dikenal: {line}");
                    }
                }
            }
        }
    });
}

fn main() {
    // Pastikan proses ini otomatis mati kalau parent process (app Tauri)
    // mati dengan cara apapun, termasuk kill -9.
    #[cfg(target_os = "linux")]
    unsafe {
        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
    }

    let verbose = std::env::args().any(|a| a == "--verbose" || a == "-v");
    let settings = Settings::default();
    let enigo = match Enigo::new(&settings) {
        Ok(e) => Arc::new(Mutex::new(e)),
        Err(e) => {
            if verbose {
                eprintln!("Gagal inisialisasi Enigo: {}", e);
            }
            std::process::exit(1);
        }
    };

    // TODO: verifikasi signature terhadap versi crate enigo 0.2 yang terpasang -
    // khususnya apakah Enigo Send+Sync di target platform (Linux/Windows) untuk
    // dibagi via Arc<Mutex<_>> antar thread. Berdasarkan dokumentasi enigo 0.2
    // ini didukung, tapi belum diverifikasi end-to-end di device fisik.
    spawn_stdin_command_listener(Arc::clone(&enigo), verbose);

    loop {
        let port = connect_loop(verbose);
        let mut reader = BufReader::new(port);
        let mut line = String::with_capacity(16);
        if verbose {
            println!("Siap. Tempelkan kartu...");
        }
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => continue,
                Ok(_) => {
                    let uid = line.trim();
                    if uid.is_empty() {
                        continue;
                    }
                    if verbose {
                        let _ = std::io::stdout().write_all(uid.as_bytes());
                        let _ = std::io::stdout().write_all(b"\n");
                    }
                    if let Ok(mut guard) = enigo.lock() {
                        let _ = guard.text(uid);
                        let _ = guard.key(enigo::Key::Return, enigo::Direction::Click);
                    }
                }
                Err(e) => {
                    if e.kind() != std::io::ErrorKind::TimedOut {
                        if verbose {
                            eprintln!("Koneksi terputus ({}). Mencoba sambung ulang...", e);
                        }
                        break;
                    }
                }
            }
        }
    }
}
