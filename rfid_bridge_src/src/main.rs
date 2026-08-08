use serialport::{SerialPort, SerialPortType};
use std::io::{BufRead, BufReader, Write};
use std::time::Duration;
use std::thread;
use enigo::{Enigo, Keyboard, Settings};

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

fn main() {
    // Pastikan proses ini otomatis mati kalau parent process (app Tauri)
    // mati dengan cara apapun, termasuk kill -9.
    #[cfg(target_os = "linux")]
    unsafe {
        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
    }

    let verbose = std::env::args().any(|a| a == "--verbose" || a == "-v");
    let settings = Settings::default();
    let mut enigo = match Enigo::new(&settings) {
        Ok(e) => e,
        Err(e) => {
            if verbose {
                eprintln!("Gagal inisialisasi Enigo: {}", e);
            }
            std::process::exit(1);
        }
    };

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
                    let _ = enigo.text(uid);
                    let _ = enigo.key(enigo::Key::Return, enigo::Direction::Click);
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
