/**
 * Sprachassistent mit Claude (Einstellungen › App › Sprachassistent: Claude).
 *
 * Claude hat keine Echtzeit-Sprachschnittstelle, deshalb eine Kette:
 *   Mikrofon (eigene Aufnahme, Satzende ueber Stille bzw. Loslassen) -> voice_stt (OpenAI gpt-transcribe)
 *   -> claude_messages (Claude mit denselben Werkzeugen wie der Realtime-Assistent, Schleife bis keine Werkzeuge mehr)
 *   -> voice_tts (OpenAI gpt-4o-mini-tts) -> abspielen.
 * Die Schluessel bleiben in Rust (voice.rs); hier entsteht nur der Gespraechsverlauf.
 * Gleiche Schnittstelle wie Voice (voice.ts): start/stop/hold, active, isPtt, isHeld, dieselben Zustaende.
 */
import { invoke } from "@tauri-apps/api/core";
import { b64, INSTRUCTIONS, PreCapture, TOOLS, type VoiceHooks, type VoiceState } from "./voice";
import { CLAUDE_IDS, type ClaudeModel } from "./settings-model";

/** nach der Antwort so lange auf eine Rueckfrage warten (Tastenkuerzel) */
const FOLLOW_MS = 8_000;
/** ohne Gesagtes nach dem Start so lange warten */
const FIRST_MS = 10_000;
/** Controller: nach der Antwort bleibt die Sitzung (unsichtbar) so lange fuer den naechsten Druck */
const READY_MS = 20_000;
/** so lange Stille nach dem Sprechen = Satz zu Ende */
const SILENCE_MS = 800;
/** laenger nimmt ein Satz nicht auf */
const MAX_MS = 20_000;
/** Pegel, ab dem es als Sprechen zaehlt (wie im Realtime-Assistenten) */
const LOUD = 0.12;
/** hoechstens so viele Werkzeug-Runden je Frage */
const MAX_ROUNDS = 6;

const SYSTEM = `${INSTRUCTIONS}
Deine Antwort wird vorgelesen: schreib so, wie man spricht, ohne Markdown, Listen, Emojis oder Links.`;

/** Werkzeuge im Format der Messages-API (Name, Beschreibung, JSON-Schema wie beim Realtime-Assistenten) */
const CLAUDE_TOOLS = TOOLS.map((t) => ({ name: t.name, description: t.description, input_schema: t.parameters }));

type Block = { type: string; id?: string; name?: string; input?: Record<string, unknown>; text?: string };
type Reply = { content: Block[]; stop_reason: string };

/** PCM16 mono 24 kHz -> WAV-Datei */
function wav(pcm: Uint8Array) {
  const out = new Uint8Array(44 + pcm.length);
  const v = new DataView(out.buffer);
  const str = (o: number, s: string) => { for (let i = 0; i < s.length; i++) out[o + i] = s.charCodeAt(i); };
  str(0, "RIFF"); v.setUint32(4, 36 + pcm.length, true); str(8, "WAVE");
  str(12, "fmt "); v.setUint32(16, 16, true); v.setUint16(20, 1, true); v.setUint16(22, 1, true);
  v.setUint32(24, 24_000, true); v.setUint32(28, 48_000, true); v.setUint16(32, 2, true); v.setUint16(34, 16, true);
  str(36, "data"); v.setUint32(40, pcm.length, true);
  out.set(pcm, 44);
  return out;
}

export class ClaudeVoice {
  state: VoiceState = "off";
  private mic?: MediaStream;
  private rec?: PreCapture;
  private ctx?: AudioContext;
  private raf = 0;
  private idle = 0;
  private ptt = false;
  private held = false;
  /** im aktuellen Satz schon gesprochen / seit wann wieder still / seit wann aufgenommen */
  private heard = false;
  private quietAt = 0;
  private recAt = 0;
  private waitMs = FIRST_MS;
  private audio?: HTMLAudioElement;
  private outCtx?: AudioContext;
  private outRaf = 0;
  private messages: unknown[] = [];
  /** laufende Runde; eine neue (Unterbrechen, Stopp) macht alte Antworten wirkungslos */
  private run = 0;

  constructor(private h: VoiceHooks, private model: () => ClaudeModel) {}

  get active() { return this.state !== "off" && this.state !== "error"; }
  get isPtt() { return this.ptt; }
  get isHeld() { return this.held; }

  private set(s: VoiceState, detail?: string) {
    this.state = s;
    this.h.onState(s, detail);
  }

  private idleIn(ms: number) {
    clearTimeout(this.idle);
    this.idle = window.setTimeout(() => this.stop(), ms);
  }

  async start() {
    if (this.active) return;
    this.messages = [];
    this.h.onTranscript("");
    this.set("connecting");
    try {
      this.mic = await navigator.mediaDevices.getUserMedia({
        audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true },
      });
    } catch (e) {
      this.fail(e);
      return;
    }
    if (!this.active) { this.stop(); return; }
    this.h.onMic?.(this.mic);
    this.meter();
    if (this.ptt && !this.held) { this.set("ready"); this.idleIn(READY_MS); return; }
    this.listen(FIRST_MS);
  }

  /** neuen Satz aufnehmen; `wait`: so lange darf es still bleiben, bevor die Sitzung endet */
  private listen(wait: number) {
    this.run++;
    this.rec?.stop();
    this.rec = this.mic ? new PreCapture(this.mic) : undefined;
    this.heard = false;
    this.quietAt = 0;
    this.recAt = Date.now();
    this.waitMs = wait;
    clearTimeout(this.idle);
    this.set("listening");
  }

  /** Halten zum Sprechen (Controller): gedrueckt = zuhoeren (unterbricht eine Antwort), losgelassen = Satz fertig */
  async hold(down: boolean) {
    this.held = down;
    if (down) {
      this.ptt = true;
      if (!this.active) { await this.start(); return; }
      this.silence();
      this.h.onTranscript("");
      this.listen(MAX_MS);
    } else if (this.state === "listening") {
      void this.sentence();
    }
  }

  /** Mikrofonpegel: Anzeige und Satzende (Stille nach dem Sprechen), ohne Gesagtes nach einer Weile Schluss */
  private meter() {
    try {
      const ctx = (this.ctx = new AudioContext());
      const an = ctx.createAnalyser();
      an.fftSize = 512;
      ctx.createMediaStreamSource(this.mic!).connect(an);
      const buf = new Uint8Array(an.fftSize);
      const loop = () => {
        an.getByteTimeDomainData(buf);
        let sum = 0;
        for (const v of buf) sum += ((v - 128) / 128) ** 2;
        const lvl = Math.min(1, Math.sqrt(sum / buf.length) * 4);
        this.h.onLevel(this.state === "listening" ? lvl : 0);
        if (this.state === "listening" && this.rec) {
          const now = Date.now();
          if (lvl > LOUD) { this.heard = true; this.quietAt = 0; }
          else if (this.heard && !this.quietAt) this.quietAt = now;
          const done = this.heard && !this.held && this.quietAt && now - this.quietAt > SILENCE_MS;
          if (done || this.rec.ms > MAX_MS) void this.sentence();
          else if (!this.heard && !this.held && now - this.recAt > this.waitMs) this.stop();
        }
        this.raf = requestAnimationFrame(loop);
      };
      loop();
    } catch {
      /* ohne Pegel kein automatisches Satzende: dann nur per Loslassen */
    }
  }

  /** Satz zu Ende: verschriftlichen, Claude fragen, Antwort vorlesen */
  private async sentence() {
    this.h.onTurnEnd?.();
    const rec = this.rec;
    this.rec = undefined;
    if (!rec) return;
    const ms = rec.ms;
    const pcm = rec.take();
    rec.stop();
    const run = ++this.run;
    if (ms < 300 || !this.heard) { this.after(); return; }
    this.set("thinking");
    try {
      const text = await invoke<string>("voice_stt", { wav: b64(wav(pcm)) });
      if (run !== this.run) return;
      if (!text) { this.after(); return; }
      this.h.onTranscript(text);
      this.messages.push({ role: "user", content: text });
      const answer = await this.converse(run);
      if (run !== this.run) return;
      await this.say(answer, run);
    } catch (e) {
      if (run === this.run) this.fail(e);
    }
  }

  /** Claude mit Werkzeugen, bis keine Werkzeuge mehr gewuenscht sind; gibt den gesprochenen Text zurueck */
  private async converse(run: number): Promise<string> {
    const model = CLAUDE_IDS[this.model()];
    // Sonnet/Opus: bei einer Ablehnung durch die Sicherheitsfilter antwortet automatisch ein anderes Modell
    const fallback = model !== CLAUDE_IDS.haiku;
    for (let i = 0; i < MAX_ROUNDS; i++) {
      const body: Record<string, unknown> = {
        model,
        max_tokens: 2048,
        system: SYSTEM,
        tools: CLAUDE_TOOLS,
        messages: this.messages,
        // kurze Sprachbefehle: wenig Nachdenken, schnelle Antwort
        output_config: { effort: "low" },
        ...(fallback ? { fallbacks: "default" } : {}),
      };
      const r = await invoke<Reply>("claude_messages", { body, beta: fallback ? "server-side-fallback-2026-07-01" : null });
      if (run !== this.run) return "";
      // ganzen Inhalt anhaengen (auch Denk-Bloecke): so will es die API fuer die naechste Runde
      this.messages.push({ role: "assistant", content: r.content });
      if (r.stop_reason === "refusal") return "Dabei kann ich leider nicht helfen.";
      const calls = r.content.filter((b) => b.type === "tool_use");
      if (r.stop_reason !== "tool_use" || !calls.length) {
        return r.content.filter((b) => b.type === "text").map((b) => b.text ?? "").join(" ").trim();
      }
      const results = [];
      for (const c of calls) {
        let out: unknown;
        try {
          const fn = this.h.tools[c.name ?? ""];
          out = fn ? await fn(c.input ?? {}) : { fehler: "unbekanntes Werkzeug" };
        } catch (e) {
          out = { fehler: String(e) };
        }
        results.push({ type: "tool_result", tool_use_id: c.id, content: JSON.stringify(out ?? { ok: true }) });
      }
      // alle Ergebnisse in einer Nachricht, sonst ruft Claude Werkzeuge nicht mehr parallel auf
      this.messages.push({ role: "user", content: results });
    }
    return "Das hat zu viele Schritte gebraucht, ich habe aufgehört.";
  }

  private async say(text: string, run: number) {
    if (!text) { this.after(); return; }
    this.h.onTranscript(text);
    const mp3 = await invoke<string>("voice_tts", { text });
    if (run !== this.run) return;
    this.set("speaking");
    const audio = (this.audio = new Audio(`data:audio/mpeg;base64,${mp3}`));
    this.outMeter(audio);
    await new Promise<void>((done) => {
      audio.onended = () => done();
      audio.onerror = () => done();
      audio.play().catch(() => done());
    });
    if (run === this.run) this.after();
  }

  /** Antwort vorbei: per Controller unsichtbar bereit, sonst kurz auf eine Rueckfrage hoeren */
  private after() {
    if (!this.active) return;
    this.h.onOutLevel?.(0);
    if (this.held) { this.listen(MAX_MS); return; }
    if (this.ptt) { this.set("ready"); this.idleIn(READY_MS); return; }
    this.listen(FOLLOW_MS);
  }

  /** laufende Wiedergabe abbrechen */
  private silence() {
    this.run++;
    cancelAnimationFrame(this.outRaf);
    this.audio?.pause();
    this.audio = undefined;
    void this.outCtx?.close().catch(() => {});
    this.outCtx = undefined;
    this.h.onOutLevel?.(0);
  }

  /** Pegel der vorgelesenen Antwort fuer die Animation in der kleinen Notch */
  private outMeter(audio: HTMLAudioElement) {
    try {
      void this.outCtx?.close().catch(() => {});
      const ctx = (this.outCtx = new AudioContext());
      const an = ctx.createAnalyser();
      an.fftSize = 512;
      const src = ctx.createMediaElementSource(audio);
      src.connect(an);
      an.connect(ctx.destination);
      const buf = new Uint8Array(an.fftSize);
      let smooth = 0;
      const loop = () => {
        an.getByteTimeDomainData(buf);
        let sum = 0;
        for (const v of buf) sum += ((v - 128) / 128) ** 2;
        smooth = Math.max(Math.min(1, Math.sqrt(sum / buf.length) * 5), smooth * 0.82);
        this.h.onOutLevel?.(smooth);
        this.outRaf = requestAnimationFrame(loop);
      };
      loop();
    } catch {
      /* ohne Pegel spielt die Antwort trotzdem */
    }
  }

  private fail(e: unknown) {
    const msg = e instanceof Error ? e.message : String(e);
    this.stop();
    this.set("error", msg.includes("Permission") || msg.includes("NotAllowed") ? "Kein Zugriff aufs Mikrofon" : msg);
  }

  stop() {
    clearTimeout(this.idle);
    cancelAnimationFrame(this.raf);
    this.silence();
    this.rec?.stop();
    this.rec = undefined;
    this.mic?.getTracks().forEach((t) => t.stop());
    this.mic = undefined;
    void this.ctx?.close().catch(() => {});
    this.ctx = undefined;
    this.ptt = this.held = false;
    this.h.onLevel(0);
    this.h.onMic?.(null);
    if (this.state !== "error") this.set("off");
  }
}
