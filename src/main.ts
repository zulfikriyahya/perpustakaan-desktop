import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { LazyStore } from "@tauri-apps/plugin-store";

const store = new LazyStore("app_config.json");
const URL_KEY = "circulation_url";

// TODO: ASUMSI - 20 detik dianggap wajar untuk "stuck tanpa jaringan" (iframe
// src ter-set tapi tidak pernah 'load', mis. DNS resolve gantung / koneksi
// putus tanpa respons HTTP error yang jelas). Belum diminta eksplisit.
const IFRAME_LOAD_TIMEOUT_MS = 20_000;

let splashEl: HTMLElement | null;
let statusTextEl: HTMLElement | null;
let formEl: HTMLFormElement | null;
let urlInputEl: HTMLInputElement | null;
let frameEl: HTMLIFrameElement | null;
let windowFocusListenerAttached = false;
let stuckWatchdogTimer: ReturnType<typeof setTimeout> | null = null;

function clearStuckWatchdog() {
  if (stuckWatchdogTimer !== null) {
    clearTimeout(stuckWatchdogTimer);
    stuckWatchdogTimer = null;
  }
}

function showSplash(message: string, prefillUrl: string | null) {
  clearStuckWatchdog();
  if (frameEl) frameEl.style.display = "none";
  if (splashEl) splashEl.style.display = "flex";
  if (statusTextEl) statusTextEl.textContent = message;
  if (urlInputEl && prefillUrl !== null) {
    urlInputEl.value = prefillUrl;
  }
}

function showContent(url: string) {
  if (!frameEl) return;
  clearStuckWatchdog();

  const isNewSrc = frameEl.getAttribute("src") !== url;
  if (isNewSrc) {
    frameEl.src = url;
  }
  frameEl.style.display = "block";
  if (splashEl) splashEl.style.display = "none";

  invoke("request_focus").catch(() => {});

  // Watchdog "stuck": kalau load tidak pernah selesai dalam waktu tertentu,
  // anggap macet dan coba reconnect ulang lewat attemptLoad() (bukan cuma
  // reload src) supaya health-check re-run juga.
  if (isNewSrc) {
    stuckWatchdogTimer = setTimeout(() => {
      showSplash("Koneksi tampak macet. Mencoba menghubungkan ulang...", url);
      void attemptLoad();
    }, IFRAME_LOAD_TIMEOUT_MS);
  }

  frameEl.addEventListener(
    "load",
    () => {
      clearStuckWatchdog();
      for (const delayMs of [0, 200, 600, 1200]) {
        setTimeout(() => frameEl?.focus(), delayMs);
      }

      // TODO: ASUMSI - koordinat klik diasumsikan tengah window (cocok dengan
      // posisi input "Tempelkan kartu atau ketik nama..." di screenshot yang
      // diberikan). Ini BELUM dikalibrasi terhadap resolusi layar kios yang
      // sebenarnya - kalau input tidak persis di tengah pada layar target,
      // klik ini bisa mengenai elemen lain. Perlu dikonfirmasi/disesuaikan.
      setTimeout(() => {
        const x = Math.round(window.innerWidth / 2);
        const y = Math.round(window.innerHeight / 2);
        invoke("send_activation_click", { x, y }).catch(() => {});
      }, 800);
    },
    { once: true }
  );

  if (!windowFocusListenerAttached) {
    windowFocusListenerAttached = true;
    getCurrentWindow()
      .onFocusChanged(({ payload: focused }) => {
        if (focused && frameEl && frameEl.style.display === "block") {
          frameEl.focus();
        }
      })
      .catch(() => {});
  }
}

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

  await listen("open-url-config", () => {
    attemptLoad(true);
  });

  await listen("force-reload", () => {
    attemptLoad();
  });

  await listen<{ reachable: boolean; url: string }>("circulation-status", (event) => {
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
