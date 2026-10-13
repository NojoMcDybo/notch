// „Hey Jarvis“-Erkennung (src/wake/detector.ts) mit WAV-Dateien pruefen: 16 kHz, mono, 16 Bit.
// Je Datei der hoechste Wert; Dateien mit pos_ im Namen muessen >= 0,5 erreichen, alle anderen darunter bleiben.
//   node --experimental-transform-types tools/wake-test.mjs <modellordner> <wav-ordner>
// onnxruntime-node: aus einem vorhandenen node_modules (ORT_NODE=…/node_modules/onnxruntime-node)
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";

const [models, wavs] = process.argv.slice(2);
const ort = createRequire(import.meta.url)(process.env.ORT_NODE ?? "onnxruntime-node");
const { WakeDetector, CHUNK } = await import(new URL("../src/wake/detector.ts", import.meta.url).href);
const read = (f) => new Uint8Array(fs.readFileSync(path.join(models, f)));
const load = () => WakeDetector.load(ort, { mel: read("melspectrogram.onnx"), emb: read("embedding_model.onnx"), wake: read("hey_jarvis_v0.1.onnx") });

function pcm(file) {
  const b = fs.readFileSync(file);
  let o = 12;
  while (o < b.length) {
    const id = b.toString("ascii", o, o + 4), n = b.readUInt32LE(o + 4);
    if (id === "fmt ") {
      const rate = b.readUInt32LE(o + 12), bits = b.readUInt16LE(o + 22), ch = b.readUInt16LE(o + 10);
      if (rate !== 16000 || bits !== 16 || ch !== 1) throw new Error(`${file}: ${rate} Hz ${bits} Bit ${ch} Kanaele`);
    }
    if (id === "data") return Float32Array.from(new Int16Array(b.buffer.slice(b.byteOffset + o + 8, b.byteOffset + o + 8 + n)));
    o += 8 + n + (n & 1);
  }
  throw new Error(`${file}: keine Audiodaten`);
}

let fehler = 0;
for (const f of fs.readdirSync(wavs).filter((f) => f.endsWith(".wav")).sort()) {
  const det = await load();
  // 1 s Stille davor und danach, wie im echten Betrieb
  const audio = new Float32Array(16000 + pcm(path.join(wavs, f)).length + 16000);
  audio.set(pcm(path.join(wavs, f)), 16000);
  let max = 0;
  for (let i = 0; i + CHUNK <= audio.length; i += CHUNK) max = Math.max(max, await det.push(audio.subarray(i, i + CHUNK)));
  const soll = f.startsWith("pos_");
  const ok = soll === max >= 0.5;
  if (!ok) fehler++;
  console.log(`${ok ? "✓" : "✗"} ${max.toFixed(3)} ${f}`);
}
console.log(fehler ? `${fehler} falsch` : "alle richtig");
process.exit(fehler ? 1 : 0);
