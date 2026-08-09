import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { LazyStore } from "@tauri-apps/plugin-store";

const store = new LazyStore("app_config.json");
const URL_KEY = "circulation_url";

let splashEl: HTMLElement | null;
let statusTextEl: HTMLElement | null;
let formEl: HTMLFormElement | null;
let urlInputEl: HTMLInputElement | null;
let frameEl: HTMLIFrameElement | null;

function showSplash(message: string, prefillUrl: string | null) {
  if (frameEl) frameEl.style.display = "none";
  if (splashEl) splashEl.style.display = "flex";
  if (statusTextEl) statusTextEl.textContent = message;
  if (urlInputEl && prefillUrl !== null) {
    urlInputEl.value = prefillUrl;
  }
}

function showContent(url: string) {
  if (!frameEl) return;
  if (frameEl.getAttribute("src") !== url) {
    frameEl.src = url;
  }
  frameEl.style.display = "block";
  if (splashEl) splashEl.style.display = "none";
}

// forceConfig = true dipakai untuk shortcut Ctrl+Shift+U, selalu tampilkan form
// terlepas dari status koneksi saat ini.
async function attemptLoad(forceConfig = false) {
  const savedUrl = (await store.get<string>(URL_KEY)) ?? null;

  if (!savedUrl) {
    showSplash("Belum ada alamat sistem sirkulasi yang diatur.", "");
    return;
  }

  if (forceConfig) {
    showSplash("Ubah alamat sistem sirkulasi jika diperlukan.", savedUrl);
    return;
  }

  showSplash("Menghubungkan ke sistem sirkulasi...", savedUrl);
  const reachable = await invoke<boolean>("check_url_reachable", { url: savedUrl });

  if (reachable) {
    showContent(savedUrl);
  } else {
    showSplash("Tidak dapat terhubung ke sistem sirkulasi. Menunggu koneksi pulih...", savedUrl);
  }
}

window.addEventListener("DOMContentLoaded", async () => {
  splashEl = document.querySelector("#splash");
  statusTextEl = document.querySelector("#status-text");
  formEl = document.querySelector("#url-form");
  urlInputEl = document.querySelector("#url-input");
  frameEl = document.querySelector("#content-frame");

  formEl?.addEventListener("submit", async (e) => {
    e.preventDefault();
    const value = urlInputEl?.value.trim();
    if (!value) return;

    await store.set(URL_KEY, value);
    await store.save();
    await attemptLoad();
  });

  await attemptLoad();

  // Shortcut Ctrl+Shift+U dari Rust - buka form ganti URL kapan saja.
  await listen("open-url-config", () => {
    attemptLoad(true);
  });

  // Shortcut Ctrl+Shift+R dari Rust - reload paksa (re-check + reload iframe).
  await listen("force-reload", () => {
    attemptLoad();
  });

  // Update berkala dari health-check monitor di Rust.
  await listen<{ reachable: boolean; url: string }>("circulation-status", (event) => {
    // Jangan ganggu operator yang sedang membuka form ganti URL secara manual.
    const isConfigOpenManually =
      formEl?.style.display === "flex" && frameEl?.style.display !== "block";
    if (isConfigOpenManually) return;

    const { reachable, url } = event.payload;
    if (reachable) {
      showContent(url);
    } else {
      showSplash("Tidak dapat terhubung ke sistem sirkulasi. Menunggu koneksi pulih...", url);
    }
  });
});
