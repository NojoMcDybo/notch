# laya-ts (eingebettet)

Aus [NandhaKishorM/laya](https://github.com/NandhaKishorM/laya) `laya-ts/src` @ fa9a2a7, Apache-2.0 (`LICENSE`).
Nur die Dateien, die `Agent` im Browser braucht. `laya-ts` ist nicht auf npm, deshalb hier als Kopie —
unverändert übernommen aus Arena (`arena/src/laya/vendor`), dort zuerst angepasst.

Anpassungen (nur `providers.ts`, mit „Anpassung:“ markiert):

1. `useWebOrt(ort)`: onnxruntime-web wird übergeben statt per `import("onnxruntime-" + "web")` geladen (für Vite unsichtbar).
2. `fetchArrayBuffer` ohne CacheStorage (das Modell liegt lokal; ein Cache legte es doppelt ab).

Die Notch übergibt `onnxruntime-web/all` (WebGPU über JSEP, passend zu `ort-wasm-simd-threaded.jsep.wasm` im Modellpaket): laya-ts legt den Encoder dann auf die GPU
(`graphOptimizationLevel: "basic"`, sonst bricht WebGPU ab) und fällt bei Fehlern selbst auf WASM zurück.

Alle Dateien tragen `// @ts-nocheck`: sie sind für Node-Typen geschrieben; geprüft wird upstream.
Aktualisieren: Dateien neu kopieren, die beiden Stellen wieder einsetzen, Modellpaket gegenprüfen (`src-tauri/src/laya.rs`).
