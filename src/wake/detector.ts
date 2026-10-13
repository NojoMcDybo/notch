/**
 * „Hey Jarvis“ erkennen: Nachbau der Streaming-Pipeline von openWakeWord (github.com/dscripka/openWakeWord,
 * openwakeword/utils.py AudioFeatures, Code Apache-2.0; Modelle CC BY-NC-SA 4.0, siehe public/wakeword/README.md).
 *
 * Je 80 ms Audio (1280 Samples, 16 kHz, Werte im int16-Bereich):
 * 1. Mel-Spektrogramm aus den neuen 1280 + 480 vorherigen Samples -> 8 Zeilen à 32, transformiert x / 10 + 2
 * 2. Einbettung aus den letzten 76 Mel-Zeilen -> 96 Werte
 * 3. Klassifikator aus den letzten 16 Einbettungen (1,28 s) -> Wert 0..1; ab 0,5 gilt das Wort als gesagt
 *
 * Ohne eigene Abhaengigkeit: `ort` wird uebergeben (onnxruntime-web im Worker, onnxruntime-node im Test).
 */
export const RATE = 16_000;
export const CHUNK = 1280;
const CONTEXT = 480;
const MEL_ROWS = 76;
const EMB_ROWS = 16;

/* eslint-disable @typescript-eslint/no-explicit-any */
type Ort = { InferenceSession: { create(model: Uint8Array): Promise<any> }; Tensor: new (type: "float32", data: Float32Array, dims: number[]) => any };

export class WakeDetector {
  private tail = new Float32Array(CONTEXT);
  /** wie openWakeWord: mit Einsen vorbelegt */
  private mel: Float32Array[] = Array.from({ length: MEL_ROWS }, () => new Float32Array(32).fill(1));
  private emb: Float32Array[] = [];

  private constructor(private ort: Ort, private melS: any, private embS: any, private wakeS: any) {}

  static async load(ort: Ort, models: { mel: Uint8Array; emb: Uint8Array; wake: Uint8Array }) {
    const [m, e, w] = await Promise.all([models.mel, models.emb, models.wake].map((b) => ort.InferenceSession.create(b)));
    return new WakeDetector(ort, m, e, w);
  }

  /** Puffer leeren (nach einem Treffer oder einer Pause), damit dasselbe Wort nicht noch einmal zaehlt */
  reset() {
    this.tail.fill(0);
    this.emb = [];
  }

  /** 1280 neue Samples -> Wert 0..1 (0, solange noch keine 1,28 s Einbettungen da sind) */
  async push(chunk: Float32Array): Promise<number> {
    if (chunk.length !== CHUNK) throw new Error(`erwartet ${CHUNK} Samples`);
    const x = new Float32Array(CONTEXT + CHUNK);
    x.set(this.tail);
    x.set(chunk, CONTEXT);
    this.tail = x.slice(CHUNK);

    const mo = await this.melS.run({ [this.melS.inputNames[0]]: new this.ort.Tensor("float32", x, [1, x.length]) });
    const md: Float32Array = mo[this.melS.outputNames[0]].data;
    for (let r = 0; r + 32 <= md.length; r += 32) this.mel.push(md.slice(r, r + 32).map((v) => v / 10 + 2));
    this.mel.splice(0, this.mel.length - MEL_ROWS);

    const win = new Float32Array(MEL_ROWS * 32);
    this.mel.forEach((row, i) => win.set(row, i * 32));
    const eo = await this.embS.run({ [this.embS.inputNames[0]]: new this.ort.Tensor("float32", win, [1, MEL_ROWS, 32, 1]) });
    this.emb.push(Float32Array.from(eo[this.embS.outputNames[0]].data as Float32Array));
    this.emb.splice(0, this.emb.length - EMB_ROWS);
    if (this.emb.length < EMB_ROWS) return 0;

    const feat = new Float32Array(EMB_ROWS * 96);
    this.emb.forEach((row, i) => feat.set(row, i * 96));
    const wo = await this.wakeS.run({ [this.wakeS.inputNames[0]]: new this.ort.Tensor("float32", feat, [1, EMB_ROWS, 96]) });
    return (wo[this.wakeS.outputNames[0]].data as Float32Array)[0];
  }
}
