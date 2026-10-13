# tools/jarvis — Laya für Jarvis bauen und messen

Plan und Ergebnisse: [`docs/jarvis.md`](../../docs/jarvis.md). Arbeitsordner (nicht im Repo): `D:\Dev\_downloads\laya-build`
mit dem Laya-Repo (`laya/`, `laya-ts` gebaut), Python-venv und den Modellordnern.

| Datei | Zweck |
|---|---|
| `saetze.json` | 51 deutsche Testsätze (nie im Training) |
| `make_train.py`, `make_notebook.py` | Trainingssatz und Kaggle-Notebook für das Feintuning (M2b) |
| `eval.mjs` | Laya auf den Testsätzen (`ABSICHT=1`: nur die trainierte Frage) |
| `early-eval.mjs` | Vorab-Entscheidung der Notch (`src/jarvis-live.ts`) Wort für Wort, Schwellen |
| `quantize.py` | fp32-Export → INT4 mit int8-Einbettung (die INT4-Tabelle rechnet auf WebGPU falsch) |
| `serve-pkg.mjs` | Paket ausliefern wie ein GitHub-Release (Range, CORS) |
| `worker-test.html` | Notch-Worker (`src/laya/worker.ts`) im Browser: Laden, Antworten, Zeit |
| `webgpu.html`, `serve-bench.mjs` | Zeitmessung M2a (WASM/WebGPU) |

## Modellpaket erneuern

1. Feingetuntes Checkpoint nach ONNX exportieren (`laya/laya-ts/scripts/export_onnx.py`, fp32).
2. `venv/Scripts/python.exe -I tools/jarvis/quantize.py <export> <ziel>`; mit `eval.mjs`/`early-eval.mjs` gegenprüfen.
3. Paketordner mit den Release-Namen aus `src-tauri/src/laya.rs` (`FILES`) anlegen, dazu `LICENSE` aus dem Laya-Repo
   und `node_modules/onnxruntime-web/dist/ort-wasm-simd-threaded.jsep.wasm` (exakt die Version aus `package.json`).
4. Größen und SHA-256 in `FILES` eintragen, `PACKAGE` ändern (alte Ordner werden dann ersetzt), neuen Release-Tag.
5. Download testen: `node tools/jarvis/serve-pkg.mjs <paketordner> 8767`, dann
   `NOTCH_LAYA_BASE=http://127.0.0.1:8767 cargo test --lib laya_download -- --ignored --nocapture` (in `src-tauri`).
6. Worker im Browser: Paket im installierten Layout (Namen auf der Platte) mit `serve-pkg.mjs <ordner> 8768`,
   `npm run dev`, `/tools/jarvis/worker-test.html` öffnen.
