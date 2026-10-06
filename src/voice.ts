/**
 * Sprachassistent: OpenAI Realtime API ueber WebRTC.
 * Rust holt einen kurzlebigen Schluessel (der echte bleibt in Rust), dann spricht diese
 * Datei direkt mit OpenAI: Mikrofon rein, Stimme raus, Ereignisse ueber den Datenkanal.
 * Der Assistent kann ueber Werkzeuge die Notch bedienen (Musik, Lautstaerke, Timer, Ablage).
 *
 * Vorspeichern: Das Mikrofon laeuft ab dem ersten Moment mit (PreCapture), auch waehrend die Verbindung noch
 * aufgebaut wird. Steht sie, geht das schon Gesagte als Audio-Eintrag hinterher (conversation.item.create
 * mit input_audio, PCM16 24 kHz); danach laeuft das Mikro live ueber WebRTC weiter. Man muss also nicht
 * warten, bis „Verbinde …“ weg ist.
 *
 * Ende: Ist die Antwort zu Ende gesprochen, schliesst der Assistent: per Controller sofort (Notch weg,
 * Verbindung legt nach READY_MS auf), per Tastenkuerzel nach kurzer Zeit fuer eine Rueckfrage (FOLLOW_MS).
 */
import { invoke } from "@tauri-apps/api/core";

/** ready = Halten-zum-Sprechen: Antwort fertig, Verbindung steht noch, Mikro aus (wartet auf den naechsten Druck) */
export type VoiceState = "off" | "connecting" | "listening" | "thinking" | "speaking" | "ready" | "error";

type Tool = (args: Record<string, unknown>) => Promise<unknown>;

export type VoiceHooks = {
  onState: (s: VoiceState, detail?: string) => void;
  onTranscript: (text: string) => void;
  onLevel: (level: number) => void;
  /** Pegel der Stimme des Assistenten (0..1) — fuer die Animation in der kleinen Notch */
  onOutLevel?: (level: number) => void;
  tools: Record<string, Tool>;
};

const CALLS_URL = "https://api.openai.com/v1/realtime/calls";
/** ohne Gespraech nach so vielen ms automatisch auflegen (Kosten laufen pro Minute) */
const IDLE_MS = 40_000;
/** Controller: nach der Antwort bleibt die Verbindung (unsichtbar) so lange fuer den naechsten Druck */
const READY_MS = 20_000;
/** Tastenkuerzel: nach der Antwort so lange auf eine Rueckfrage hoeren, dann schliessen */
const FOLLOW_MS = 8_000;
/** Vorspeichern: hoechstens so viel Sprache vor dem Verbindungsaufbau aufheben */
const PRE_MAX_S = 30;
/** unter so viel Vorgespeichertem (Knacken, Luft) wird nichts nachgereicht */
const PRE_MIN_MS = 250;
/** Antwort fertig, aber kein "Ausgabe gestoppt" vom Server: nach so langer Stille gilt sie als zu Ende */
const QUIET_MS = 1200;
/** Halten-zum-Sprechen: haengt "denkt nach" so lange, ohne dass etwas kommt, wird aufgegeben */
const THINK_MAX_MS = 20_000;

const INSTRUCTIONS = `Du bist der Sprachassistent in Nojos Notch auf seinem Windows-PC.
Sprich Deutsch, locker und knapp: meist ein, hoechstens zwei Saetze, ausser er will ausdruecklich mehr.
Mit den Werkzeugen steuerst du Musik, Lautstaerke, Timer und die Datei-Ablage, kannst nachsehen, was die Notch gerade zeigt, und kennst die laufenden Spielstaende (Werkzeug sport) und laufende League-of-Legends-Profispiele mit Gold, Kills, Objectives und den Spielern je Lane (Werkzeug esports).
Nutze die Werkzeuge, statt zu behaupten, du haettest etwas getan. Wenn etwas unklar ist, frag kurz nach.
Blutzuckerwerte in der Notch stammen aktuell aus einer Demo: nenne sie nie als echte Messung und gib keine Therapie- oder Dosierungsempfehlungen.`;

const TOOLS = [
  {
    type: "function",
    name: "musik",
    description: "Die Musik steuern, die gerade in Windows laeuft (z. B. Spotify, Browser).",
    parameters: {
      type: "object",
      properties: { aktion: { type: "string", enum: ["play", "pause", "toggle", "next", "prev"] } },
      required: ["aktion"],
    },
  },
  {
    type: "function",
    name: "lautstaerke",
    description: "Systemlautstaerke in Prozent setzen (0 bis 100).",
    parameters: { type: "object", properties: { prozent: { type: "number" } }, required: ["prozent"] },
  },
  {
    type: "function",
    name: "timer_starten",
    description: "Einen Timer stellen.",
    parameters: { type: "object", properties: { minuten: { type: "number" } }, required: ["minuten"] },
  },
  {
    type: "function",
    name: "timer_stoppen",
    description: "Den laufenden Timer beenden.",
    parameters: { type: "object", properties: {} },
  },
  {
    type: "function",
    name: "status",
    description:
      "Was die Notch gerade zeigt: laufende Musik, Eintraege von Apps (offenes PDF, Timer, Blutzucker-Demo …), Dateien in der Ablage, Uhrzeit.",
    parameters: { type: "object", properties: {} },
  },
  {
    type: "function",
    name: "esports",
    description:
      "League-of-Legends-Profispiele aus Vantage: Teams, Spielzeit, Gold, Kills, Tuerme, Inhibitoren, Barone, Drachen, Serienstand und je Lane Spieler, Champion, K/D/A, CS, Level und Goldvorsprung (oder Draft, wenn das Spiel noch nicht begonnen hat).",
    parameters: { type: "object", properties: {} },
  },
  {
    type: "function",
    name: "sport",
    description:
      "Live-Sport aus der Notch: laufende, kommende und gerade beendete Spiele der gewaehlten Wettbewerbe (Bundesliga, Laenderspiele …) mit Spielstand, Minute und den letzten Meldungen (Tore, Karten).",
    parameters: { type: "object", properties: {} },
  },
  {
    type: "function",
    name: "datei_oeffnen",
    description: "Eine Datei aus der Ablage der Notch mit dem passenden Programm oeffnen.",
    parameters: {
      type: "object",
      properties: { name: { type: "string", description: "Dateiname oder ein Teil davon" } },
      required: ["name"],
    },
  },
  {
    type: "function",
    name: "datei_konvertieren",
    description: "Eine Datei aus der Ablage umwandeln. Das Ergebnis landet neben dem Original.",
    parameters: {
      type: "object",
      properties: {
        name: { type: "string", description: "Dateiname oder ein Teil davon" },
        ziel: { type: "string", description: "Zielformat, z. B. png, jpg, webp, gif, mp3, mp4, wav" },
      },
      required: ["name", "ziel"],
    },
  },
];

/**
 * Nimmt das Mikrofon auf, bis die Verbindung steht: Mono, auf 24 kHz umgerechnet, PCM16 (Format der Realtime-API).
 * Laeuft auf einer Kopie der Spur, damit das Abschalten der Sendespur die Aufnahme nicht stumm macht.
 */
class PreCapture {
  private ctx: AudioContext;
  private node: ScriptProcessorNode;
  private chunks: Int16Array[] = [];
  private len = 0;
  private phase = 0;

  constructor(stream: MediaStream) {
    this.ctx = new AudioContext();
    void this.ctx.resume().catch(() => {});
    const src = this.ctx.createMediaStreamSource(stream);
    this.node = this.ctx.createScriptProcessor(4096, 1, 1);
    const ratio = this.ctx.sampleRate / 24_000;
    this.node.onaudioprocess = (e) => {
      const inp = e.inputBuffer.getChannelData(0);
      const out = new Int16Array(Math.ceil(inp.length / ratio) + 1);
      let n = 0;
      let t = this.phase;
      for (; t < inp.length - 1; t += ratio) {
        const i = Math.floor(t), f = t - i;
        const v = Math.max(-1, Math.min(1, inp[i] * (1 - f) + inp[i + 1] * f));
        out[n++] = v < 0 ? v * 0x8000 : v * 0x7fff;
      }
      this.phase = Math.max(0, t - inp.length);
      if (this.len + n > PRE_MAX_S * 24_000) return;
      this.chunks.push(out.subarray(0, n));
      this.len += n;
    };
    src.connect(this.node);
    this.node.connect(this.ctx.destination); // ohne Ziel laeuft der Prozessor nicht (gibt nur Stille aus)
  }

  /** bisher Aufgenommenes (Bytes PCM16 little-endian) */
  get ms() {
    return (this.len / 24_000) * 1000;
  }

  take(): Uint8Array {
    const all = new Int16Array(this.len);
    let o = 0;
    for (const c of this.chunks) { all.set(c, o); o += c.length; }
    this.chunks = [];
    this.len = 0;
    return new Uint8Array(all.buffer);
  }

  stop() {
    this.node.onaudioprocess = null;
    this.node.disconnect();
    void this.ctx.close().catch(() => {});
  }
}

function b64(bytes: Uint8Array) {
  let s = "";
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s);
}

export class Voice {
  private pc?: RTCPeerConnection;
  private dc?: RTCDataChannel;
  private mic?: MediaStream;
  /** Kopie der Mikrofonspur, die an OpenAI geht (an/aus beim Halten); das Original speist Pegel und Vorspeichern */
  private sendTrack?: MediaStreamTrack;
  private pre?: PreCapture;
  /** zuletzt laut gesprochen (lokaler Pegel) — ob man beim Verbinden noch mitten im Satz ist */
  private loudAt = 0;
  /** seit wann das Mikro live an OpenAI geht (kurzer Rest = nichts zum Abschliessen) */
  private liveAt = 0;
  /** in dieser Runde wurde schon Vorgespeichertes nachgereicht */
  private sentPre = false;
  private ctx?: AudioContext;
  private audio = new Audio();
  private raf = 0;
  private idle = 0;
  private text = "";
  /** Sprechen per Knopfdruck (Controller): keine automatische Spracherkennung, Mikro nur beim Halten */
  private ptt = false;
  /** gehalten = Mikro soll an sein (auch waehrend die Verbindung noch aufgebaut wird) */
  private held = false;
  /** Server spielt gerade Stimme ab (output_audio_buffer.started … stopped) */
  private playing = false;
  /** letzte Antwort ist fertig erzeugt (response.done ohne Werkzeugaufruf) */
  private answered = false;
  private outCtx?: AudioContext;
  private outRaf = 0;
  private quietSince = 0;
  private thinkTimer = 0;
  state: VoiceState = "off";

  /** Sitzung laeuft per Halten-zum-Sprechen (Controller) */
  get isPtt() {
    return this.ptt;
  }

  /** Taste wird gerade gehalten */
  get isHeld() {
    return this.held;
  }

  constructor(private h: VoiceHooks) {
    this.audio.autoplay = true;
  }

  get active() {
    return this.state !== "off" && this.state !== "error";
  }

  toggle() {
    return this.active ? this.stop() : this.start();
  }

  private set(s: VoiceState, detail?: string) {
    this.state = s;
    clearTimeout(this.thinkTimer);
    // Halten-zum-Sprechen: kommt auf eine Frage gar nichts zurueck, nicht ewig "denkt nach" zeigen
    if (s === "thinking" && this.ptt) this.thinkTimer = window.setTimeout(() => this.finish(), THINK_MAX_MS);
    this.h.onState(s, detail);
  }

  /**
   * Antwort ist zu Ende gesprochen.
   * Controller: Mikro aus, Notch weg, Verbindung wartet unsichtbar READY_MS auf den naechsten Druck.
   * Tastenkuerzel: noch FOLLOW_MS fuer eine Rueckfrage zuhoeren, dann schliessen.
   */
  private finish() {
    if (!this.active || this.held || this.state === "connecting") return;
    this.answered = false;
    this.playing = false;
    if (this.ptt) {
      this.set("ready");
      this.idleIn(READY_MS);
      return;
    }
    this.set("listening");
    this.idleIn(FOLLOW_MS);
  }

  private send(ev: unknown) {
    if (this.dc?.readyState === "open") this.dc.send(JSON.stringify(ev));
  }

  private bumpIdle() {
    this.idleIn(IDLE_MS);
  }

  private idleIn(ms: number) {
    clearTimeout(this.idle);
    this.idle = window.setTimeout(() => this.stop(), ms);
  }

  /** Halten zum Sprechen: gedrueckt = zuhoeren (startet die Sitzung bei Bedarf), losgelassen = antworten */
  async hold(down: boolean) {
    this.held = down;
    if (down) {
      if (!this.active) {
        this.ptt = true;
        await this.start();
        return;
      }
      // Verbindung steht noch nicht: das Vorspeichern laeuft weiter, configure() uebernimmt
      if (this.state === "connecting") return;
      if (!this.ptt) {
        // Sitzung lief mit automatischer Erkennung (Tastenkuerzel): ab jetzt per Knopfdruck
        this.ptt = true;
        this.configure();
      }
      // spricht der Assistent gerade: unterbrechen
      if (this.state === "speaking" || this.state === "thinking" || this.playing) {
        this.send({ type: "response.cancel" });
        this.send({ type: "output_audio_buffer.clear" });
      }
      this.answered = this.playing = this.sentPre = false;
      this.send({ type: "input_audio_buffer.clear" });
      this.setMic(true);
      this.text = "";
      this.h.onTranscript("");
      this.set("listening");
      this.bumpIdle();
    } else {
      if (!this.ptt || this.state === "connecting" || !this.active) return; // configure() kuemmert sich
      this.release();
    }
  }

  private release() {
    const live = Date.now() - this.liveAt;
    this.setMic(false);
    // nur abschliessen, wenn live wirklich etwas ankam (sonst „Puffer leer“) — das Vorgespeicherte ist schon drin
    if (!this.sentPre || live > 300) this.send({ type: "input_audio_buffer.commit" });
    this.send({ type: "response.create" });
    this.set("thinking");
    this.bumpIdle();
  }

  private setMic(on: boolean) {
    if (this.sendTrack) this.sendTrack.enabled = on;
    if (on) this.liveAt = Date.now();
  }

  /**
   * Schon Gesagtes nachreichen: als Audio-Eintrag(e) des Nutzers. Ein Datenkanal-Paket darf nicht beliebig gross
   * sein, also in Stuecke von hoechstens ~2,5 s. Gibt zurueck, ob etwas Hoerbares dabei war.
   */
  private flushPre(): boolean {
    const pre = this.pre;
    this.pre = undefined;
    if (!pre) return false;
    const ms = pre.ms;
    const pcm = pre.take();
    pre.stop();
    if (ms < PRE_MIN_MS || !this.loudAt) return false;
    const limit = Math.min(120_000, Math.floor((((this.pc?.sctp?.maxMessageSize || 65_536) - 4096) * 3) / 4));
    const step = Math.max(24_000, limit - (limit % 2));
    for (let i = 0; i < pcm.length; i += step) {
      this.send({
        type: "conversation.item.create",
        item: { type: "message", role: "user", content: [{ type: "input_audio", audio: b64(pcm.subarray(i, i + step)) }] },
      });
    }
    this.sentPre = true;
    return true;
  }

  async start() {
    if (this.active) return;
    this.text = "";
    this.h.onTranscript("");
    this.loudAt = 0;
    this.sentPre = false;
    this.set("connecting");
    try {
      // Mikro zuerst und gleich mitschneiden: man darf sofort losreden, nachgereicht wird beim Verbinden
      this.mic = await navigator.mediaDevices.getUserMedia({
        audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true },
      });
      this.meter(this.mic);
      try { this.pre = new PreCapture(this.mic); } catch { /* ohne Vorspeichern: wie bisher erst nach dem Verbinden */ }
      if (!this.active) { this.stop(); return; } // waehrenddessen abgebrochen: Mikro wieder freigeben
      const token = await invoke<string>("voice_token");
      if (!this.active) { this.stop(); return; }
      const pc = (this.pc = new RTCPeerConnection());
      pc.ontrack = (e) => {
        this.audio.srcObject = e.streams[0];
        this.outMeter(e.streams[0]);
      };
      // an OpenAI geht eine Kopie der Spur; aus, bis das Vorgespeicherte nachgereicht ist
      this.sendTrack = this.mic.getAudioTracks()[0].clone();
      this.sendTrack.enabled = !this.pre;
      pc.addTrack(this.sendTrack, this.mic);

      const dc = (this.dc = pc.createDataChannel("oai-events"));
      dc.onopen = () => this.configure();
      dc.onmessage = (e) => this.onEvent(JSON.parse(e.data));

      const offer = await pc.createOffer();
      await pc.setLocalDescription(offer);
      const r = await fetch(CALLS_URL, {
        method: "POST",
        body: offer.sdp,
        headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/sdp" },
      });
      if (!r.ok) throw new Error(`OpenAI ${r.status}: ${(await r.text()).slice(0, 160)}`);
      await pc.setRemoteDescription({ type: "answer", sdp: await r.text() });
      pc.onconnectionstatechange = () => {
        if (["failed", "closed"].includes(pc.connectionState)) this.stop();
      };
    } catch (e) {
      this.stop();
      const msg = e instanceof Error ? e.message : String(e);
      this.set("error", msg.includes("Permission") || msg.includes("NotAllowed") ? "Kein Zugriff aufs Mikrofon" : msg);
    }
  }

  stop() {
    clearTimeout(this.idle);
    clearTimeout(this.thinkTimer);
    cancelAnimationFrame(this.raf);
    cancelAnimationFrame(this.outRaf);
    this.dc?.close();
    this.pc?.close();
    this.mic?.getTracks().forEach((t) => t.stop());
    this.sendTrack?.stop();
    this.pre?.stop();
    this.pre = this.sendTrack = undefined;
    void this.ctx?.close().catch(() => {});
    void this.outCtx?.close().catch(() => {});
    this.audio.srcObject = null;
    this.dc = this.pc = this.mic = this.ctx = this.outCtx = undefined;
    this.ptt = this.held = this.playing = this.answered = this.sentPre = false;
    this.h.onLevel(0);
    this.h.onOutLevel?.(0);
    if (this.state !== "error") this.set("off");
  }

  private configure() {
    this.send({
      type: "session.update",
      session: {
        type: "realtime",
        instructions: INSTRUCTIONS,
        tools: TOOLS,
        tool_choice: "auto",
        // Knopfdruck: wir sagen selbst, wann der Satz fertig ist (commit beim Loslassen)
        audio: { input: { turn_detection: this.ptt ? null : { type: "semantic_vad" } } },
      },
    });
    // was waehrend des Verbindens schon gesagt wurde, hinterherschicken
    const had = this.flushPre();
    this.bumpIdle();
    if (this.ptt) {
      if (this.held) {
        // haelt noch: live weiter, Loslassen schliesst ab
        this.setMic(true);
        this.set("listening");
      } else if (had) {
        // schon losgelassen: das Vorgespeicherte beantworten
        this.send({ type: "response.create" });
        this.set("thinking");
      } else {
        this.set("listening");
        this.release();
      }
      return;
    }
    // Tastenkuerzel: live weiter (die Spracherkennung des Servers merkt das Satzende). War man beim Verbinden
    // schon still, gleich antworten lassen — sonst wartet der Server auf Sprache, die nicht mehr kommt.
    this.setMic(true);
    this.set("listening");
    if (had && Date.now() - this.loudAt > 700) {
      this.send({ type: "response.create" });
      this.set("thinking");
    }
  }

  private onEvent(ev: { type: string; [k: string]: any }) {
    switch (ev.type) {
      case "input_audio_buffer.speech_started":
        this.text = "";
        this.answered = false;
        this.set("listening");
        this.bumpIdle();
        break;
      case "input_audio_buffer.speech_stopped":
        this.set("thinking");
        break;
      case "response.output_audio_transcript.delta":
        this.text += ev.delta ?? "";
        this.h.onTranscript(this.text);
        if (this.state !== "speaking" && !(this.ptt && this.held)) this.set("speaking");
        break;
      // WebRTC: wann die Stimme wirklich laeuft (die Antwort ist oft schon fertig erzeugt, waehrend sie noch spricht)
      case "output_audio_buffer.started":
        this.playing = true;
        break;
      case "output_audio_buffer.stopped":
      case "output_audio_buffer.cleared":
        this.playing = false;
        if (this.answered) this.finish();
        break;
      case "response.done":
        void this.done(ev.response);
        break;
      case "error":
        // leerer Puffer, aber das Vorgespeicherte ist schon drin: die Antwort kommt trotzdem
        if (this.sentPre && String(ev.error?.code ?? "").includes("buffer")) break;
        // Halten-zum-Sprechen ohne Gesagtes (leerer Puffer) o. Ae.: still zurueck auf bereit
        if (this.ptt && !this.held && this.state === "thinking") {
          this.answered = true;
          if (!this.playing) this.finish();
          break;
        }
        // Fehler einer einzelnen Anfrage: anzeigen, Sitzung aber weiterlaufen lassen
        this.h.onTranscript(`Fehler: ${ev.error?.message ?? "unbekannt"}`);
        break;
    }
  }

  /** Antwort fertig: Werkzeugaufrufe ausfuehren und das Ergebnis zurueckgeben. */
  private async done(resp: { output?: any[]; status?: string } | undefined) {
    this.bumpIdle();
    const calls = (resp?.output ?? []).filter((o) => o.type === "function_call");
    if (!calls.length) {
      this.sentPre = false;
      // abgebrochen, weil schon wieder gedrueckt bzw. dazwischengeredet wurde: es hoert bereits zu
      if (this.held || resp?.status === "cancelled") return;
      // warten, bis die Stimme zu Ende ist (output_audio_buffer.stopped oder Stille), dann schliessen
      this.answered = true;
      this.quietSince = 0;
      if (!this.playing && this.state !== "speaking") this.finish();
      return;
    }
    this.set("thinking");
    for (const c of calls) {
      let out: unknown;
      try {
        const fn = this.h.tools[c.name];
        out = fn ? await fn(JSON.parse(c.arguments || "{}")) : { fehler: "unbekanntes Werkzeug" };
      } catch (e) {
        out = { fehler: String(e) };
      }
      this.send({
        type: "conversation.item.create",
        item: { type: "function_call_output", call_id: c.call_id, output: JSON.stringify(out ?? { ok: true }) },
      });
    }
    this.text = "";
    this.send({ type: "response.create" });
  }

  /**
   * Pegel der Antwort-Stimme: treibt die Animation in der kleinen Notch und erkennt das Ende der
   * Antwort, falls der Server kein output_audio_buffer.stopped schickt (Stille nach response.done).
   */
  private outMeter(stream: MediaStream) {
    cancelAnimationFrame(this.outRaf);
    try {
      const ctx = (this.outCtx = new AudioContext());
      const an = ctx.createAnalyser();
      an.fftSize = 512;
      ctx.createMediaStreamSource(stream).connect(an);
      const buf = new Uint8Array(an.fftSize);
      let smooth = 0;
      const loop = () => {
        an.getByteTimeDomainData(buf);
        let sum = 0;
        for (const v of buf) sum += ((v - 128) / 128) ** 2;
        const lvl = Math.min(1, Math.sqrt(sum / buf.length) * 5);
        smooth = Math.max(lvl, smooth * 0.82);
        this.h.onOutLevel?.(smooth);
        if (this.answered && !this.held) {
          const now = performance.now();
          if (lvl > 0.03) this.quietSince = 0;
          else if (!this.quietSince) this.quietSince = now;
          else if (now - this.quietSince > QUIET_MS) this.finish();
        }
        this.outRaf = requestAnimationFrame(loop);
      };
      loop();
    } catch {
      /* ohne Pegel: Ende kommt ueber output_audio_buffer.stopped */
    }
  }

  /** Mikrofonpegel fuer die Animation */
  private meter(stream: MediaStream) {
    try {
      const ctx = (this.ctx = new AudioContext());
      const an = ctx.createAnalyser();
      an.fftSize = 512;
      ctx.createMediaStreamSource(stream).connect(an);
      const buf = new Uint8Array(an.fftSize);
      const loop = () => {
        an.getByteTimeDomainData(buf);
        let sum = 0;
        for (const v of buf) sum += ((v - 128) / 128) ** 2;
        const lvl = Math.min(1, Math.sqrt(sum / buf.length) * 4);
        if (lvl > 0.12) this.loudAt = Date.now();
        this.h.onLevel(lvl);
        this.raf = requestAnimationFrame(loop);
      };
      loop();
    } catch {
      /* ohne Pegelanzeige geht es auch */
    }
  }
}
