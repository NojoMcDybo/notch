/**
 * Sprachassistent: OpenAI Realtime API ueber WebRTC.
 * Rust holt einen kurzlebigen Schluessel (der echte bleibt in Rust), dann spricht diese
 * Datei direkt mit OpenAI: Mikrofon rein, Stimme raus, Ereignisse ueber den Datenkanal.
 * Der Assistent kann ueber Werkzeuge die Notch bedienen (Musik, Lautstaerke, Timer, Ablage).
 */
import { invoke } from "@tauri-apps/api/core";

export type VoiceState = "off" | "connecting" | "listening" | "thinking" | "speaking" | "error";

type Tool = (args: Record<string, unknown>) => Promise<unknown>;

export type VoiceHooks = {
  onState: (s: VoiceState, detail?: string) => void;
  onTranscript: (text: string) => void;
  onLevel: (level: number) => void;
  tools: Record<string, Tool>;
};

const CALLS_URL = "https://api.openai.com/v1/realtime/calls";
/** ohne Gespraech nach so vielen ms automatisch auflegen (Kosten laufen pro Minute) */
const IDLE_MS = 40_000;

const INSTRUCTIONS = `Du bist der Sprachassistent in Nojos Notch auf seinem Windows-PC.
Sprich Deutsch, locker und knapp: meist ein, hoechstens zwei Saetze, ausser er will ausdruecklich mehr.
Mit den Werkzeugen steuerst du Musik, Lautstaerke, Timer und die Datei-Ablage und kannst nachsehen, was die Notch gerade zeigt.
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

export class Voice {
  private pc?: RTCPeerConnection;
  private dc?: RTCDataChannel;
  private mic?: MediaStream;
  private ctx?: AudioContext;
  private audio = new Audio();
  private raf = 0;
  private idle = 0;
  private text = "";
  /** Sprechen per Knopfdruck (Controller): keine automatische Spracherkennung, Mikro nur beim Halten */
  private ptt = false;
  /** gehalten = Mikro soll an sein (auch waehrend die Verbindung noch aufgebaut wird) */
  private held = false;
  state: VoiceState = "off";

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
    this.h.onState(s, detail);
  }

  private send(ev: unknown) {
    if (this.dc?.readyState === "open") this.dc.send(JSON.stringify(ev));
  }

  private bumpIdle() {
    clearTimeout(this.idle);
    this.idle = window.setTimeout(() => this.stop(), IDLE_MS);
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
      if (!this.ptt) {
        // Sitzung lief mit automatischer Erkennung (Tastenkuerzel): ab jetzt per Knopfdruck
        this.ptt = true;
        this.configure();
      }
      // spricht der Assistent gerade: unterbrechen
      if (this.state === "speaking" || this.state === "thinking") {
        this.send({ type: "response.cancel" });
        this.send({ type: "output_audio_buffer.clear" });
      }
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
    this.setMic(false);
    this.send({ type: "input_audio_buffer.commit" });
    this.send({ type: "response.create" });
    this.set("thinking");
    this.bumpIdle();
  }

  private setMic(on: boolean) {
    this.mic?.getAudioTracks().forEach((t) => { t.enabled = on; });
  }

  async start() {
    if (this.active) return;
    this.text = "";
    this.h.onTranscript("");
    this.set("connecting");
    try {
      const token = await invoke<string>("voice_token");
      const pc = (this.pc = new RTCPeerConnection());
      pc.ontrack = (e) => { this.audio.srcObject = e.streams[0]; };
      this.mic = await navigator.mediaDevices.getUserMedia({
        audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true },
      });
      pc.addTrack(this.mic.getAudioTracks()[0], this.mic);
      this.meter(this.mic);

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
    cancelAnimationFrame(this.raf);
    this.dc?.close();
    this.pc?.close();
    this.mic?.getTracks().forEach((t) => t.stop());
    void this.ctx?.close().catch(() => {});
    this.audio.srcObject = null;
    this.dc = this.pc = this.mic = this.ctx = undefined;
    this.ptt = this.held = false;
    this.h.onLevel(0);
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
    this.set("listening");
    this.bumpIdle();
    // schon losgelassen, waehrend die Verbindung aufgebaut wurde: das Gesagte trotzdem beantworten
    if (this.ptt && !this.held) this.release();
  }

  private onEvent(ev: { type: string; [k: string]: any }) {
    switch (ev.type) {
      case "input_audio_buffer.speech_started":
        this.text = "";
        this.set("listening");
        this.bumpIdle();
        break;
      case "input_audio_buffer.speech_stopped":
        this.set("thinking");
        break;
      case "response.output_audio_transcript.delta":
        this.text += ev.delta ?? "";
        this.h.onTranscript(this.text);
        if (this.state !== "speaking") this.set("speaking");
        break;
      case "response.done":
        void this.done(ev.response);
        break;
      case "error":
        // Fehler einer einzelnen Anfrage: anzeigen, Sitzung aber weiterlaufen lassen
        this.h.onTranscript(`Fehler: ${ev.error?.message ?? "unbekannt"}`);
        break;
    }
  }

  /** Antwort fertig: Werkzeugaufrufe ausfuehren und das Ergebnis zurueckgeben. */
  private async done(resp: { output?: any[] } | undefined) {
    this.bumpIdle();
    const calls = (resp?.output ?? []).filter((o) => o.type === "function_call");
    if (!calls.length) {
      this.set("listening");
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
        this.h.onLevel(Math.min(1, Math.sqrt(sum / buf.length) * 4));
        this.raf = requestAnimationFrame(loop);
      };
      loop();
    } catch {
      /* ohne Pegelanzeige geht es auch */
    }
  }
}
