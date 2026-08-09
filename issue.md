Berikut konversi dokumen aturan tersebut, disesuaikan dari konteks Laravel+Filament ke konteks project **Tauri + Rust + RFID Bridge** (`perpustakaan-mtsn1pandeglang`) yang sudah kita pelajari strukturnya:

---

# Aturan

## Konteks Proyek
- Aplikasi desktop **Tauri 2** (`perpustakaan-mtsn1pandeglang`) yang berfungsi sebagai **shell kios** untuk mengakses sistem sirkulasi perpustakaan MTsN 1 Pandeglang. Frontend bukan SPA custom — window Tauri langsung me-load URL live (`https://perpustakaan.mtsn1pandeglang.sch.id/dashboard/sirkulasi`) dalam mode fullscreen tanpa decoration. Logic bisnis sirkulasi (state machine peminjaman, dsb.) berada di **server web terpisah**, bukan di codebase Tauri ini.
- Komponen utama: (1) `src-tauri` — backend Rust (Tauri core: single-instance, autostart, global shortcut, lifecycle sidecar), (2) `rfid_bridge_src` — binary Rust terpisah yang dikompilasi jadi sidecar (`binaries/rfid_bridge-*`), bertugas membaca UID dari ESP32 RFID reader via serial lalu menyuntikkannya sebagai keystroke (via `enigo`) ke window yang sedang fokus, (3) `src` — frontend TypeScript vanilla (saat ini masih template default, belum banyak dipakai karena window langsung load URL eksternal).
- Struktur folder eksisting (lihat tree project) adalah acuan konvensi — **jangan** memperkenalkan pola folder baru (mis. memecah `src-tauri/src` jadi banyak module tanpa alasan eksplisit, atau menambah binary sidecar kedua) tanpa alasan eksplisit.
- Jika ada perubahan yang berdampak ke **web app sirkulasi** (server terpisah, di luar repo ini) — misalnya format UID yang dikirim, event keyboard yang disimulasikan, atau endpoint yang diakses — itu ikut jadi sumber kebenaran tambahan dan wajib diselaraskan, bukan ditebak dari kode Tauri saja.

## 1. Acuan Utama
Semua implementasi mengikuti struktur dan konvensi kode existing secara ketat. Jika ada bagian ambigu (mis. behavior pasti saat device RFID terputus di tengah sesi, threshold reconnect, format UID yang diharapkan web app: dengan/tanpa Enter, uppercase/lowercase, dsb.), **jangan menebak diam-diam** — tandai eksplisit dengan `// TODO: ASUMSI - ...` beserta alasan.

## 2. Hindari Emoticon
Kode, komentar, pesan commit, log output (`println!`/`eprintln!` di `rfid_bridge` maupun `lib.rs`) tetap formal.

## 3. Prinsip DRY - Satu Sumber Kebenaran
- Konstanta identitas device RFID (VID/PID yang dikenali: CP2102, CH340, CH9102, dsb.) **hanya** didefinisikan satu tempat di `rfid_bridge_src` — jangan duplikasi list VID/PID di tempat lain.
- Logika koneksi/reconnect serial (`connect_loop`, `find_esp32_port`) terpusat — kode lain yang butuh status koneksi device memanggil fungsi ini, bukan menulis ulang loop serial sendiri.
- Logika lifecycle sidecar (spawn, kill saat window close, kill saat shortcut Ctrl+Shift+Q) **hanya** di `src-tauri/src/lib.rs` via `RfidBridgeState` — jangan spawn/kill sidecar dari tempat lain (mis. dari command Tauri baru yang terpisah) tanpa melalui state yang sama, supaya tidak ada dua proses sidecar berjalan bersamaan.
- Konfigurasi window (fullscreen, decorations, URL target) terpusat di `tauri.conf.json` — jangan override behavior ini secara ad-hoc dari kode Rust/TS di tempat lain kecuali memang untuk logic runtime (mis. auto-fullscreen saat fokus pertama, yang memang sudah ada di `setup()`).
- Jika nanti ditambah command Tauri baru untuk komunikasi frontend↔backend (`invoke`), definisikan sekali di `lib.rs` dan pastikan hanya satu titik pendaftaran command.

## 4. Komentar Singkat
Komentar hanya sebagai penanda ringkas (mis. `// sidecar di-kill saat window close`, `// deteksi via VID/PID dulu, fallback ke nama port`), bukan narasi panjang.

## 5. Tandai Gap dengan TODO
Setiap keputusan sepihak (mis. format persis UID yang dikirim ke web app, apakah perlu delay antar keystroke di `enigo`, strategi retry kalau `enigo` gagal inisialisasi, apakah shortcut keluar perlu konfirmasi dialog) wajib ditandai `// TODO: GAP-SPEC - ...` di lokasi kode terkait.

## 6. Tidak Ada File Placeholder Kosong
Jika menambah module Rust baru (mis. `src-tauri/src/rfid.rs` terpisah dari `lib.rs`) atau file TS baru, setiap file wajib berisi kode valid dan sudah di-`mod`/`import` dengan benar — jangan menyerahkan stub kosong yang membuat `cargo build`/`tsc` gagal.

## 7. Verifikasi API/Package Eksternal
Untuk pemanggilan method dari crate/package eksternal yang dipakai project (`tauri`, `tauri-plugin-shell`, `tauri-plugin-global-shortcut`, `tauri-plugin-autostart`, `tauri-plugin-single-instance`, `serialport`, `enigo`, `@tauri-apps/api`), verifikasi signature terhadap versi di `Cargo.toml`/`Cargo.lock` atau `package.json` **sebelum** menuliskan kode. Jika tidak bisa diverifikasi:
```rust
// TODO: verifikasi signature terhadap versi crate yang terpasang
```
Berlaku khusus untuk **kontrak keystroke injection** (`enigo`) dan **kontrak serial protocol** ke ESP32 (baud rate 115200, format baris UID) — jangan berasumsi dari kode lama karena ini memutus komunikasi dengan device fisik/firmware yang sudah terpasang di lapangan jika salah.

## 8. Kembangkan Bertahap, Build Harus Lolos
Setiap iterasi diasumsikan langsung dijalankan lewat `npm run tauri dev` (frontend+backend) dan/atau `cargo build --release` khusus untuk `rfid_bridge_src`. Jangan menyerahkan kode yang jelas belum lengkap (fungsi dipanggil tapi belum didefinisikan, `use` hilang, sidecar binary belum di-build ulang setelah `rfid_bridge_src` diubah, permission `capabilities/default.json` belum diupdate untuk command baru) tanpa peringatan eksplisit bahwa aplikasi akan error dan alasannya.

## 9. Perbaikan Kecil → Full Fungsi/Method
Jika perubahan hanya sedikit, kirim versi lengkap fungsi/struct beserta lokasi file (mis. `src-tauri/src/lib.rs`, fungsi `kill_sidecar`).

## 10. Perbaikan Besar → File Penuh
Jika perubahan signifikan (mis. refactor lifecycle sidecar, restructure `rfid_bridge_src/src/main.rs` jadi multi-module, ubah cara window handling), kirim keseluruhan file terkait, bukan potongan parsial.

## 11. Telusuri Semua Pemakaian Simbol
Jika nama field/fungsi diganti (mis. rename `RfidBridgeState`, ganti nama sidecar binary, ubah nama shortcut kombinasi), telusuri dan perbaiki **semua** titik pemakaian: `lib.rs`, `tauri.conf.json` (`externalBin`), `capabilities/default.json` (permission `shell:allow-spawn`), `main.rs`, dan referensi di `rfid_bridge_src` bila relevan, dalam balasan yang sama.

## 12. Verifikasi Berlapis Sebelum Menyatakan "Selesai"
Jangan menyatakan fitur "final"/"solid" hanya berdasarkan tinjauan statis. Nyatakan status jujur:
- Apakah sudah di-build (`cargo build`, `npm run tauri build`) oleh pengguna?
- Apakah sudah diuji end-to-end dengan device fisik (ESP32 + RFID reader ditancapkan, kartu ditempelkan, UID benar-benar masuk ke form web app)?
- Apakah lifecycle sidecar sudah diverifikasi (tidak ada proses `rfid_bridge` menggantung/zombie setelah app ditutup, terutama via `kill -9` parent)?
- Apakah shortcut Ctrl+Shift+Q dan autostart sudah diverifikasi jalan di OS target (Windows/Linux)?
- Apa saja asumsi yang masih menunggu konfirmasi (mis. format UID yang diharapkan web app)?

## 13. Target Akhir
Aplikasi kios stabil, tidak menimbulkan regresi pada mekanisme otomatis (auto-fullscreen, autostart, single-instance lock, kill sidecar saat close), serta integrasi RFID bridge (deteksi device, reconnect otomatis, injeksi keystroke UID) tetap kompatibel dengan device/firmware ESP32 yang sudah aktif di lapangan.

## 14. Struktur Balasan Konsisten
0. **(Jika file referensi belum ada di sesi)** Daftar perintah `cat` untuk file yang perlu dilihat — hentikan balasan di sini sampai isi file diberikan, jangan lanjut menebak ke poin 1-4 di bawah.
1. Ringkasan singkat perubahan/penambahan.
2. Kode (sesuai poin 9/10).
3. Daftar file/module yang terdampak (termasuk apakah perlu update `tauri.conf.json`, `capabilities/default.json`, atau rebuild binary sidecar).
4. Status verifikasi (poin 12) — termasuk gap mana yang tertutup dan mana yang masih terbuka.

## 15. Jangan Berasumsi Environment
Jika versi Tauri, target OS (Windows/Linux/macOS), atau perilaku device ESP32/firmware memengaruhi solusi, tanyakan dulu daripada menebak — kecuali sudah dinyatakan sebelumnya dalam sesi. **Termasuk**: apakah web app sirkulasi (`perpustakaan.mtsn1pandeglang.sch.id`) punya kontrak input tertentu untuk form scan (mis. field harus auto-focus, format Enter setelah UID, dsb.) yang perlu diselaraskan dengan cara `rfid_bridge` mengirim keystroke.

## 16. Perubahan Kontrak Serial/Keystroke Wajib Eksplisit
Jika perbaikan mengubah cara `rfid_bridge` membaca serial atau menyuntik keystroke (baud rate, format UID yang dikirim, delay antar karakter, penambahan/penghapusan tombol Enter), harus dinyatakan eksplisit dampaknya — karena berpotensi memutus input ke web app sirkulasi yang sudah berjalan di sekolah secara real-time.

## 17. Kontrak Keamanan & Kompatibilitas Device Mengikat
- Setiap perubahan pada permission sidecar (`capabilities/default.json`, `shell:allow-spawn`), `externalBin` di `tauri.conf.json`, atau cara sidecar di-spawn/di-kill harus dinyatakan dampaknya — sidecar yang gagal start/mati prematur berarti RFID reader tidak berfungsi sama sekali di kios.
- Setiap perubahan pada CSP (`security.csp`, saat ini `null`) atau URL window target harus dinyatakan dampak keamanannya, mengingat window me-load web app eksternal secara langsung.
- Setiap perubahan global shortcut (Ctrl+Shift+Q) harus dinyatakan dampaknya terhadap operator kios yang mengandalkan shortcut ini untuk keluar dari mode fullscreen.
- Jika device fisik (ESP32 + RFID reader) sama dipakai di beberapa titik/kios, perubahan kontrak serial apa pun **mengikat semua instalasi** dan wajib dinyatakan eksplisit sebelum ditulis.

Penyimpangan dari poin di atas dianggap bug kritis, bukan perbaikan kosmetik.

## 18. Minta Referensi File Sebelum Menjawab Gap
Sebelum mulai menganalisis/menulis kode untuk sebuah issue/gap yang diajukan, **jangan langsung menebak isi file terkait dari nama/struktur folder saja**. Tentukan dulu file-file mana yang relevan untuk dilihat, lalu minta isinya dalam bentuk perintah `cat` yang bisa langsung dijalankan pengguna, contoh:

```bash
cat src-tauri/src/lib.rs
cat rfid_bridge_src/src/main.rs
cat src-tauri/tauri.conf.json
cat src-tauri/capabilities/default.json
```

Ketentuan:
- Kelompokkan dalam **satu blok bash** agar bisa dijalankan sekali jalan, jangan diminta satu-satu bolak-balik kecuali file lanjutan baru diketahui butuh setelah melihat isi file pertama.
- Prioritaskan file yang **langsung** disebut/terdampak oleh gap, lalu file yang **terhubung**.
- Jika gap juga menyinggung kontrak dengan web app sirkulasi eksternal (lihat poin 15/16), nyatakan bahwa verifikasi kontrak tersebut butuh akses ke kode/dokumentasi web app itu (di luar repo Tauri ini) — jangan berasumsi format form input-nya.
- Jika file yang diminta ternyata tidak ada / pengguna belum bisa memberikan, jangan berasumsi isinya — nyatakan bahwa keputusan/kode masih tertunda sampai isi file tersedia (kecuali kasusnya memang file baru yang akan dibuat dari nol).
- Pengecualian: jika pengguna sudah melampirkan isi file yang relevan di pesan sebelumnya dalam sesi yang sama (seperti `lib.rs`, `main.rs`, `rfid_bridge_src/src/main.rs`, `tauri.conf.json`, dsb. yang sudah dibagikan), tidak perlu meminta ulang — cukup konfirmasi singkat bahwa referensi tersebut masih dipakai.

---

# Fitur/Gap yang ingin ditutup pada iterasi ini

- tambahkan shortcut refresh (Tombol F5 atau Ctrl + Shift + R)
- Ubah Tombol Keluar (Ctrl + Shift + W)
- Tambahkan Fitur Auto Refresh Jika Server Berstatus 500x

---

Lanjutkan/selesaikan implementasi proyek ini sesuai seluruh aturan di atas. Untuk setiap gap, jika penyelesaiannya memerlukan keputusan desain yang berdampak ke **kontrak serial/keystroke RFID bridge** (poin 16), **permission sidecar/CSP/window** (poin 17), atau **kompatibilitas device/firmware ESP32** yang sudah terpasang di lapangan, **tanyakan secara eksplisit sebelum menulis kode** — jangan menebak lalu menyerahkan perubahan yang berisiko memutus koneksi device, mengganggu operator kios, atau merusak input yang mengalir ke web app sirkulasi production.
