/**
 * „Hey Jarvis“ im eigenen Thread: bekommt 16-kHz-Audio in Stuecken zu 1280 Samples (Werte im int16-Bereich) und
 * meldet, wenn das Wort gesagt wurde. Modelle: public/wakeword/ (openWakeWord), Laufzeit: onnxruntime-web (WASM, CPU).
 *
 * Nachrichten: { type: "chunk", data: Float32Array } -> bei Treffer { type: "wake", score }
 *              -> nach dem Laden { type: "ready" } | { type: "error", message }
 */
import * as ort from "onnxruntime-web/wasm";
import wasmUrl from "onnxruntime-web/ort-wasm-simd-threaded.wasm?url";
import { WakeDetector } from "./detector";

/** ab diesem Wert gilt das Wort als gesagt (openWakeWord-Standard; Gegenproben in tools/wake-test.mjs bis 0,39) */
const THRESHOLD = 0.5;
/** nach einem Treffer so lange nichts mehr melden */
const QUIET_MS = 2000;

let det: WakeDetector | null = null;
let last = 0;
const queue: Float32Array[] = [];
let busy = false;
const post = (m: unknown) => (self as unknown as Worker).postMessage(m);

async function init() {
  try {
    ort.env.wasm.numThreads = 1;
    ort.env.wasm.proxy = false;
    ort.env.wasm.wasmPaths = { wasm: wasmUrl };
    const get = async (f: string) => new Uint8Array(await (await fetch(new URL(`/wakeword/${f}`, self.location.origin))).arrayBuffer());
    det = await WakeDetector.load(ort, { mel: await get("melspectrogram.onnx"), emb: await get("embedding_model.onnx"), wake: await get("hey_jarvis_v0.1.onnx") });
    post({ type: "ready" });
  } catch (e) {
    post({ type: "error", message: String((e as Error)?.message ?? e) });
  }
}

async function drain() {
  if (busy || !det) return;
  busy = true;
  // laeuft die Erkennung hinterher (Rechner ausgelastet), alte Stuecke verwerfen statt immer weiter zurueckzuliegen
  if (queue.length > 25) queue.splice(0, queue.length - 25);
  while (queue.length) {
    const score = await det.push(queue.shift()!);
    if (score >= THRESHOLD && Date.now() - last > QUIET_MS) {
      last = Date.now();
      det.reset();
      queue.length = 0;
      post({ type: "wake", score });
    }
  }
  busy = false;
}

self.onmessage = (e: MessageEvent) => {
  if (e.data?.type === "chunk") {
    queue.push(e.data.data);
    void drain();
  } else if (e.data?.type === "reset") {
    det?.reset();
    queue.length = 0;
  }
};

void init().then(drain);
