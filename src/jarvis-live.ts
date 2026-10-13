/**
 * Jarvis, Schritt M2c (docs/jarvis.md): schon beim Sprechen handeln.
 *
 * Parallel zum Sprachassistenten laeuft eine zweite, reine Mitschrift-Sitzung bei OpenAI (gpt-live-transcribe
 * ueber WebRTC, Teiltexte waehrend des Sprechens, App-Namen als Stichwoerter). Jeder Teiltext geht durch:
 *
 * 1. Korrekturwoerter („nicht“, „nein“, „ich meine“ …): in diesem Satz nichts mehr vorab starten; was schon
 *    vorab gestartet wurde, wieder schliessen (jarvis_undo, nur solange frisch). Den Rest macht der Assistent.
 * 2. Katalog (Rust, jarvis_hint): ist ein Programm *ganz* genannt? Sonst nichts tun („Öffne Chr“).
 * 3. Laya (src/laya/worker.ts, feingetunt): „starten“ / „wechseln“ / „web“ / „nichts“?
 * 4. Auslösen bei „starten“/„wechseln“, wenn Laya sehr sicher ist oder zweimal hintereinander dasselbe sagt.
 *
 * Nur ein Vorab-Schritt je Satz (Stufe 1: starten oder nach vorn holen). Der Assistent bekommt danach den ganzen
 * Satz wie bisher; startet er dasselbe Programm, merkt jarvis.rs, dass es frisch gestartet ist (kein Doppelstart).
 *
 * An nur, wenn das Modell geladen ist (Einstellungen › App › „Schon beim Sprechen handeln“, laya.rs).
 */
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const CALLS_URL = "https://api.openai.com/v1/realtime/calls";
/** ab dieser Sicherheit sofort, darunter erst beim zweiten gleichen Teiltext. Gemessen mit tools/jarvis/early-eval.mjs
 * (51 Saetze): 0,9 startet 4 von 21 erst mit dem Endtext, 0,8 alle beim Namen, 0,7 nimmt mehr wieder zurueck. */
export const SURE = 0.8;
export const MIN_P = 0.6;

/** Woerter, nach denen im Satz nichts mehr vorab passiert (Verneinung, Korrektur) — Laya ist da schwach (Issue #377) */
export const KORREKTUR = /(^|[^\p{L}])(nicht|nein|nee|doch nicht|stattdessen|lass mal|lieber|ich meine|sondern|warte|halt)(?![\p{L}])/iu;

export type Absicht = "starten" | "wechseln" | "web" | "nichts";
export type Step = { kind: "none" } | { kind: "undo" | "starten" | "wechseln"; app: string };

/** Entscheidung je Satz, ohne Seiteneffekte (tools/jarvis/early-eval.mjs prueft sie mit echten Saetzen) */
export class Turn {
  /** vorab gestartet / nach vorn geholt */
  done?: string;
  blocked = false;
  private last = "";

  /** Schritt 1: Korrektur im Teiltext? */
  korrektur(text: string): Step {
    if (this.blocked || !KORREKTUR.test(text)) return { kind: "none" };
    this.blocked = true;
    return this.done ? { kind: "undo", app: this.done } : { kind: "none" };
  }

  /** Schritte 2–4 schon geklaert? Dann lohnt kein weiterer Laya-Durchlauf */
  get closed() {
    return this.blocked || !!this.done;
  }

  /** Schritt 4: Laya-Antwort zum Teiltext mit Programm `app` (null = kein Programm genannt) */
  laya(app: string | null, choice: Absicht, p: number): Step {
    if (this.closed || !app || (choice !== "starten" && choice !== "wechseln") || p < MIN_P) {
      this.last = "";
      return { kind: "none" };
    }
    const key = `${choice}:${app}`;
    if (p < SURE && this.last !== key) {
      this.last = key;
      return { kind: "none" };
    }
    this.done = app;
    return { kind: choice, app };
  }
}

type LayaStatus = { want: boolean; state: string; dir: string };
type Pending = (r: { choice: Absicht; p: number } | null) => void;

/** Mitschrift + Laya + Ausfuehren; main.ts gibt das Mikrofon (onMic) und das Satzende (turnEnd) weiter */
export class Jarvis {
  private worker?: Worker;
  private ready = false;
  private asks = new Map<number, Pending>();
  private askId = 0;
  private pc?: RTCPeerConnection;
  private dc?: RTCDataChannel;
  private track?: MediaStreamTrack;
  private text = new Map<string, string>();
  private item = "";
  private turn = new Turn();
  /** abgeschlossene Saetze: spaet eintreffende Mitschrift dazu nicht noch einmal auswerten */
  private ended = new Set<string>();
  /** gerade laeuft ein Durchlauf; neuere Teiltexte warten (nur der neueste zaehlt) */
  private busy = false;
  private queued?: { item: string; text: string };

  constructor() {
    void listen<LayaStatus>("laya", (e) => this.status(e.payload));
    invoke<LayaStatus>("laya_status").then((s) => this.status(s), () => {});
  }

  /** Modell bereit: Worker laden und halten (das Laden dauert Sekunden — beim ersten Satz waere es zu spaet) */
  private status(s: LayaStatus) {
    if (s.state === "ready" && s.dir && !this.worker) {
      const w = (this.worker = new Worker(new URL("./laya/worker.ts", import.meta.url), { type: "module" }));
      w.onmessage = (e) => {
        const m = e.data;
        if (m.type === "ready") this.ready = true;
        else if (m.type === "answer" || m.type === "error") {
          if (m.type === "error") console.warn("Jarvis/Laya:", m.message);
          const r = this.asks.get(m.id);
          this.asks.delete(m.id);
          r?.(m.type === "answer" ? { choice: m.choice, p: m.p } : null);
        }
      };
      w.postMessage({ type: "load", base: convertFileSrc(s.dir) });
    } else if (s.state !== "ready" && this.worker) {
      this.worker.terminate();
      this.worker = undefined;
      this.ready = false;
      this.asks.forEach((r) => r(null));
      this.asks.clear();
      this.stop();
    }
  }

  private ask(text: string) {
    return new Promise<{ choice: Absicht; p: number } | null>((res) => {
      if (!this.worker || !this.ready) return res(null);
      const id = ++this.askId;
      this.asks.set(id, res);
      this.worker.postMessage({ type: "ask", id, text });
    });
  }

  /** Mikrofon der Sprachsitzung (null = Sitzung zu): Mitschrift starten bzw. beenden */
  onMic(stream: MediaStream | null) {
    if (!stream) return this.stop();
    if (this.ready && !this.pc) void this.start(stream);
  }

  /** Satz zu Ende (Server-VAD, Loslassen, Claude-Satzende): Mitschrift abschliessen, naechster Satz neu */
  turnEnd() {
    this.send({ type: "input_audio_buffer.commit" });
    if (this.item) this.ended.add(this.item);
    this.turn = new Turn();
    this.item = "";
  }

  private send(ev: unknown) {
    if (this.dc?.readyState === "open") this.dc.send(JSON.stringify(ev));
  }

  private async start(stream: MediaStream) {
    try {
      const [token, keywords] = await Promise.all([
        invoke<string>("voice_token", { transcribe: true }),
        invoke<string[]>("jarvis_keywords").catch(() => [] as string[]),
      ]);
      if (this.pc || !stream.active) return;
      const pc = (this.pc = new RTCPeerConnection());
      this.track = stream.getAudioTracks()[0].clone();
      pc.addTrack(this.track, stream);
      const dc = (this.dc = pc.createDataChannel("oai-events"));
      dc.onopen = () =>
        this.send({
          type: "session.update",
          session: {
            type: "transcription",
            audio: {
              input: {
                transcription: { model: "gpt-live-transcribe", languages: ["de"], delay: "low", keywords },
                turn_detection: null,
              },
            },
          },
        });
      dc.onmessage = (e) => this.onEvent(JSON.parse(e.data));
      const offer = await pc.createOffer();
      await pc.setLocalDescription(offer);
      const r = await fetch(CALLS_URL, {
        method: "POST",
        body: offer.sdp,
        headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/sdp" },
      });
      if (!r.ok) throw new Error(`OpenAI ${r.status}: ${(await r.text()).slice(0, 160)}`);
      if (this.pc !== pc) return;
      await pc.setRemoteDescription({ type: "answer", sdp: await r.text() });
    } catch (e) {
      // ohne Mitschrift geht alles wie bisher, nur nicht vorab
      console.warn("Jarvis-Mitschrift:", e);
      this.stop();
    }
  }

  stop() {
    this.dc?.close();
    this.pc?.close();
    this.track?.stop();
    this.dc = this.pc = this.track = undefined;
    this.text.clear();
    this.ended.clear();
    this.turn = new Turn();
    this.item = "";
  }

  private onEvent(ev: { type: string; item_id?: string; delta?: string; transcript?: string; error?: { message?: string } }) {
    if (ev.type === "conversation.item.input_audio_transcription.delta" && ev.item_id) {
      const t = (this.text.get(ev.item_id) ?? "") + (ev.delta ?? "");
      this.text.set(ev.item_id, t);
      this.feed(ev.item_id, t);
    } else if (ev.type === "conversation.item.input_audio_transcription.completed" && ev.item_id) {
      this.text.delete(ev.item_id);
      this.feed(ev.item_id, ev.transcript ?? "");
    } else if (ev.type === "error") {
      console.warn("Jarvis-Mitschrift:", ev.error?.message);
    }
  }

  /** neuer Teiltext; laeuft noch ein Durchlauf, wird nur der neueste nachgeholt */
  private feed(item: string, text: string) {
    if (this.busy) { this.queued = { item, text }; return; }
    this.busy = true;
    void this.check(item, text).finally(() => {
      this.busy = false;
      const q = this.queued;
      this.queued = undefined;
      if (q) this.feed(q.item, q.text);
    });
  }

  private async check(item: string, text: string) {
    if (this.ended.has(item)) return;
    // neuer Eintrag in der Mitschrift = neuer Satz (falls das Satzende nicht gemeldet wurde)
    if (item !== this.item) {
      if (this.item) this.turn = new Turn();
      this.item = item;
    }
    const turn = this.turn;
    const k = turn.korrektur(text);
    if (k.kind === "undo") {
      await invoke("jarvis_undo", { name: k.app }).catch(() => {});
      return;
    }
    if (turn.closed) return;
    const hint = await invoke<{ name: string } | null>("jarvis_hint", { text }).catch(() => null);
    if (!hint) { turn.laya(null, "nichts", 0); return; }
    const a = await this.ask(text);
    if (!a || turn !== this.turn) return;
    const step = turn.laya(hint.name, a.choice, a.p);
    if (step.kind === "starten") await invoke("jarvis_app", { name: step.app }).catch(() => {});
    else if (step.kind === "wechseln") await invoke("jarvis_window", { name: step.app }).catch(() => {});
  }
}
