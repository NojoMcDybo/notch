/**
 * Laya im eigenen Thread (Web Worker) fuer Jarvis: laedt das feingetunte Modell aus dem lokalen Ordner
 * (Asset-Protokoll, src-tauri/src/laya.rs) und beantwortet je Teiltext eine Frage: Will der Sprecher gerade ein
 * Programm starten, zu einem Fenster wechseln, eine Seite oeffnen — oder nichts davon?
 * Encoder auf der GPU (WebGPU), Rueckfall auf WASM macht laya-ts selbst.
 *
 * Nachrichten: { type: "load", base } -> { type: "ready", gpu } | { type: "error", message }
 *              { type: "ask", id, text } -> { type: "answer", id, choice, p, ms }
 */

// "all" nutzt die WebGPU-Ausfuehrung ueber JSEP (ort-wasm-simd-threaded.jsep.wasm, so in M2a gemessen); das Paket
// "webgpu" braeuchte stattdessen die asyncify-Variante der WASM-Datei
import * as ort from "onnxruntime-web/all";
import { useWebOrt } from "./vendor/providers";
import { Agent, type ChoiceAnswer } from "./vendor/agent";

// laya-ts waehlt den Browser-Weg, wenn es `window` gibt; im Worker heisst das globale Objekt `self`
(globalThis as unknown as { window?: unknown }).window ??= globalThis;

const ORT_VERSION = "1.30.0";

/** Genau so trainiert (tools/jarvis/make_train.py, FRAGE) — Wortlaut nicht aendern, sonst stimmt das Modell nicht mehr */
const ABSICHT = {
  absicht: {
    type: "choice",
    instructions: "Was will der Sprecher gerade vom PC?",
    criteria: {
      starten: "ein Programm jetzt öffnen oder starten",
      wechseln: "zu einem schon offenen Fenster wechseln oder es nach vorn holen",
      web: "eine Webseite öffnen oder im Internet suchen",
      nichts: "nichts davon: eine Frage, eine Bemerkung, Musik, Timer, oder etwas zurücknehmen",
    },
  },
};

let agent: Agent | null = null;
const post = (m: unknown) => (self as unknown as Worker).postMessage(m);

self.onmessage = async (e: MessageEvent) => {
  const m = e.data;
  try {
    if (m.type === "load") {
      ort.env.wasm.numThreads = 1; // ohne Cross-Origin-Isolation keine WASM-Threads; der Encoder laeuft ohnehin auf der GPU
      ort.env.wasm.proxy = false;
      // die WASM-Datei (mit WebGPU) im Paket gehoert zu genau dieser Version (package.json exakt, laya.rs Paketkennung)
      if (ort.env.versions.web !== ORT_VERSION) throw new Error(`Modellpaket passt nicht zur App (ONNX Runtime ${ort.env.versions.web}, Paket ${ORT_VERSION})`);
      const res = await fetch(`${m.base}/ort.wasm`);
      if (!res.ok) throw new Error(`ort.wasm: ${res.status}`);
      ort.env.wasm.wasmBinary = await res.arrayBuffer();
      useWebOrt(ort);
      agent = await Agent.load(m.base);
      // einmal aufwaermen: der erste Durchlauf baut die GPU-Programme und dauert deutlich laenger
      await agent.predict("Öffne Chrome", ABSICHT);
      post({ type: "ready", gpu: "gpu" in navigator });
    } else if (m.type === "ask") {
      if (!agent) throw new Error("Modell nicht geladen");
      const t = performance.now();
      const a = (await agent.predict(m.text, ABSICHT)).answers.absicht as ChoiceAnswer;
      post({ type: "answer", id: m.id, choice: a.choice, p: a.probabilities[a.choice] ?? 0, ms: performance.now() - t });
    }
  } catch (err) {
    post({ type: "error", id: m?.id, message: String((err as Error)?.message ?? err) });
  }
};
