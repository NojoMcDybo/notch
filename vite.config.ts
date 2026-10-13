import { defineConfig, type Plugin } from "vite";
// @ts-expect-error type error without @types/node package
import process from "node:process";
const host = process.env.TAURI_DEV_HOST;

/**
 * Die WASM-Laufzeit von onnxruntime-web (28 MB) gehoert zum optionalen Laya-Modellpaket fuer Jarvis (laya.rs laedt
 * sie mit, der Worker uebergibt sie als wasmBinary). Vite wuerde sie sonst in jede Installation packen. (wie in Arena)
 */
const dropOrtWasm = (): Plugin => ({
  name: "notch-drop-ort-wasm",
  apply: "build",
  generateBundle(_, bundle) {
    for (const k of Object.keys(bundle)) if (/ort-wasm-.*\.wasm$/.test(k)) delete bundle[k];
  },
});

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [dropOrtWasm()],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // Laya-Worker (src/laya/worker.ts) als ES-Modul
  worker: { format: "es" as const },
  // zwei Seiten: die Notch selbst und das Einstellungsfenster
  build: {
    rollupOptions: {
      input: { main: "index.html", settings: "settings.html" },
    },
  },
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
