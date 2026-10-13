/**
 * „Hey Jarvis“: Mikrofon (lokal, nichts verlaesst den PC) -> 16 kHz -> Worker (worker.ts) -> onWake.
 * Laeuft nur, solange es in den Einstellungen an ist und der Assistent nicht selbst das Mikrofon hat
 * (sonst wuerde er sich mit seiner eigenen Stimme wecken). main.ts schaltet mit `set(on)`.
 */
import { CHUNK, RATE } from "./detector";

export class Wake {
  private worker?: Worker;
  private mic?: MediaStream;
  private ctx?: AudioContext;
  private node?: ScriptProcessorNode;
  private buf = new Float32Array(CHUNK);
  private fill = 0;
  private want = false;
  private starting = false;

  constructor(private onWake: () => void) {}

  /** an/aus; mehrfach aufrufbar */
  set(on: boolean) {
    this.want = on;
    if (on) void this.start();
    else this.stop();
  }

  private async start() {
    if (this.mic || this.starting) return;
    this.starting = true;
    try {
      if (!this.worker) {
        this.worker = new Worker(new URL("./worker.ts", import.meta.url), { type: "module" });
        this.worker.onmessage = (e) => {
          if (e.data?.type === "wake") this.onWake();
          else if (e.data?.type === "error") console.warn("Hey Jarvis:", e.data.message);
        };
      }
      const mic = await navigator.mediaDevices.getUserMedia({ audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true } });
      if (!this.want) { mic.getTracks().forEach((t) => t.stop()); return; }
      this.mic = mic;
      // der Browser rechnet das Mikrofon selbst auf 16 kHz um
      const ctx = (this.ctx = new AudioContext({ sampleRate: RATE }));
      void ctx.resume().catch(() => {});
      const node = (this.node = ctx.createScriptProcessor(2048, 1, 1));
      node.onaudioprocess = (e) => {
        const inp = e.inputBuffer.getChannelData(0);
        for (let i = 0; i < inp.length; i++) {
          this.buf[this.fill++] = Math.max(-1, Math.min(1, inp[i])) * 32767;
          if (this.fill === CHUNK) {
            const data = this.buf;
            this.worker?.postMessage({ type: "chunk", data }, [data.buffer]);
            this.buf = new Float32Array(CHUNK);
            this.fill = 0;
          }
        }
      };
      ctx.createMediaStreamSource(mic).connect(node);
      node.connect(ctx.destination); // ohne Ziel laeuft der Prozessor nicht (gibt nur Stille aus)
    } catch (e) {
      console.warn("Hey Jarvis: kein Mikrofon", e);
    } finally {
      this.starting = false;
    }
  }

  private stop() {
    if (this.node) this.node.onaudioprocess = null;
    this.node?.disconnect();
    void this.ctx?.close().catch(() => {});
    this.mic?.getTracks().forEach((t) => t.stop());
    this.node = this.ctx = this.mic = undefined;
    this.fill = 0;
    // naechstes Mal frisch anfangen (kein halbes Wort von vorhin)
    this.worker?.postMessage({ type: "reset" });
  }
}
