import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { startDrag } from "@crabnebula/tauri-plugin-drag";
import { Voice, type VoiceState } from "./voice";
import { Wheel } from "./wheel";
import { fillCard, isBg, type ChartData } from "./glucose";
import { build, needed, plan, planKey, planSig, PulseGate, sourceOf, type Plan } from "./compact";
import { DEFAULTS, normalize, type CompactSettings } from "./settings-model";
import { MusicReactor, type Spectrum } from "./music-react";
import { liquid } from "./nojo/nojo-ui";
import { bannerEl, cardEl, cardSig, kickoff, midEl, midSig as sportSig, Pitch, type SportMatch, type SportNews, type SportPlay, type SportState } from "./sport";

// ---------- Typen ----------

type Media = {
  title: string; artist: string; album: string; source: string;
  playing: boolean; position: number; duration: number;
  can_seek: boolean; can_next: boolean; can_prev: boolean; sessions: number; key: string;
} | null;

/** "SpotifyAB.SpotifyMusic_…!Spotify" -> "Spotify", "chrome.exe" -> "Chrome" */
function appName(source: string) {
  const s = source.replace(/^.*!/, "").replace(/\.exe$/i, "");
  return s.charAt(0).toUpperCase() + s.slice(1);
}

type Action = { id: string; label: string; icon?: string; open?: string; post?: string };
type Activity = {
  id: string; app: string; title: string; subtitle?: string; value?: string; unit?: string;
  icon?: string; color?: string; progress?: number; priority: number; alert: boolean;
  open?: string; actions: Action[]; updated: number;
  /** Countdown-Ende (ms seit 1970) -> Balken laeuft fluessig */
  ends_at?: number;
  /** Eingabefeld in der Zeile, z. B. Folios Suche */
  input?: { placeholder: string; value?: string };
  /** Herzfrequenz: Symbol schlaegt in diesem Takt (siehe Pulse-Taktgeber) */
  pulse?: number;
  /** Verlaufs-Activities (Blutzucker aus Haze), siehe glucose.ts */
  trend?: string;
  delta?: number;
  chart?: ChartData;
  pid?: number;
  expired?: boolean;
};
type Item = { path: string; name: string; ext: string; size: number; kind: string; added: number };
type Clip = {
  id: number; kind: "text" | "image" | "files"; text?: string; path?: string;
  files: string[]; width: number; height: number; at: number;
};
type Target = { id: string; label: string };
type Volume = { level: number; muted: boolean; device?: string; follows_player?: boolean };

type State = "idle" | "compact" | "clip" | "expanded";
type View = "home" | "tray" | "timer";

type Dock = "top" | "left" | "right";

const EAR = 10;
const EAR_X = 14;
const MAX_H = 680; // Fenster ist 700 hoch (lib.rs WIN_H)
/** oben: waagerecht wie die Mac-Notch */
const SIZE = {
  idle: { w: 190, h: 30, r: 10 },
  compact: { w: 320, h: 34, r: 13 },
  /** kurz beim Kopieren */
  clip: { w: 300, h: 46, r: 17 },
  expanded: { w: 520, h: 0, r: 30 },
};
/** seitlich: senkrechte Pille, aufgeklappt ein hohes Panel mit allem auf einmal */
const SIDE = {
  idle: { w: 30, h: 180, r: 10 },
  compact: { w: 44, h: 280, r: 15 },
  clip: { w: 250, h: 54, r: 18 },
  expanded: { w: 400, h: 0, r: 28 },
};
/** Einstellungen der kompakten Anzeige (Rangliste + Extras), siehe settings-model.ts */
let settings: CompactSettings = normalize(DEFAULTS);
/** so lange bleibt die Musik nach Pause noch in der kleinen Notch */
const pauseLinger = () => settings.music.lingerSec * 1000;

// ---------- DOM ----------

const q = <T extends Element = HTMLElement>(s: string, root: ParentNode = document) =>
  root.querySelector(s) as T;

const notch = q("#notch");
const lead = q(".c-lead");
const trail = q(".c-trail");
const expanded = q(".expanded");
const player = q(".player");
const cover = q<HTMLImageElement>(".cover");
const idleInfo = q(".idle-info");
const actsEl = q(".acts");
const seekEl = q(".seek");
const volEl = q(".vol");
const volSlider = q(".volume");
const trayView = q(".v-tray");
const filesEl = q(".files");
const fileBar = q(".file-bar");
const badge = q(".badge");
const clearTray = q(".clear-tray");
const shareBtn = q(".share-btn");

// ---------- Zustand ----------

let media: Media = null;
let mediaAt = 0;
let pausedAt = 0;
let coverSrc: string | null = null;
let acts: Activity[] = [];
let shelf: Item[] = [];
let clips: Clip[] = [];
let sel = new Set<string>();
let anchor: string | null = null;
/** laufende Umwandlung -> Statusleiste unten in der Ablage */
type Conv = { total: number; since: number; label: string; done: Map<string, string>; failed: Map<string, string>; hideAt: number };
let conv: Conv | null = null;
let volume: Volume = { level: 0.5, muted: false };
let hover = false;
let hoverRaw = false;
let peekUntil = 0;
let fullscreen = false;
/** im Vollbild per Maus an der Kante ausgefahren (Einstellung „Im Vollbild: Am Rand einblenden“) */
let peek = false;
/** Controller (gamepad.rs): Steuerkreuz links + RB/RT holt die Notch im Vollbild klein/aufgeklappt heraus */
let padMode: "off" | "compact" | "expanded" = "off";
/** Sprachassistent per Controller (Halten zum Sprechen): aufgeklappt nur beim Sprechen, danach klein, dann weg */
let padVoice = false;
/** Steuerkreuz links + R3 wird gehalten, aber die 1,5 s sind noch nicht um: Ladering in der Mitte */
let armUntil = 0;
let armMs = 0;
let armTimer = 0;
const arming = () => armUntil > 0;
/** Fehler/fehlender Schluessel beim Controller-Sprechen: so lange sichtbar, dann wieder weg */
let padErrUntil = 0;
/** Controller-Sprachassistent braucht die Notch gerade (Ladering, Zuhoeren, Antwort, Fehler) */
const padVisible = () => arming() || (padVoice && voiceMode() !== "off") || Date.now() < padErrUntil;
/** Vollbild: versteckt, ausser „Sichtbar, nicht anklickbar“, per Kante/Controller herausgeholt oder Sprachassistent per Controller */
const hiddenByFullscreen = () =>
  fullscreen && !peek && padMode === "off" && settings.fullscreen.mode !== "show" &&
  !((padVisible() || sportFlash()) && settings.fullscreen.mode !== "hide");
/** Vollbild: Klicks gehen durch die Notch ans Programm (versteckt oder „nicht anklickbar“) */
const passiveInFullscreen = () => fullscreen && !peek;
let dragOver = false;
let holding = 0; // laufende Zieh-Gesten (Spulen/Lautstaerke) halten die Notch offen
let state: State = "idle";
let view: View = "home";
let hoverTimer = 0;
let dragIcon = "";
let dock: Dock = "top";
/** zuletzt gemeldeter Zustand des Sprachassistenten (oder "nokey") */
let voiceUi: VoiceState | "nokey" = "off";
let voiceKey = "";

// ---------- Sprachassistent ----------

const voiceEl = q(".voice");
const orb = q(".orb");
const micBtn = q(".mic-btn");
const V_LABEL: Record<string, string> = {
  connecting: "Hört zu · verbindet …",
  listening: "Hört zu",
  thinking: "Denkt nach …",
  speaking: "Spricht",
  error: "Fehler",
  nokey: "OpenAI-Schlüssel fehlt",
};

/**
 * Wie zeigt sich der Assistent gerade?
 * - panel: aufgeklappt mit Orb und Text (normales Gespraech; beim Controller nur, solange gehalten wird)
 * - mini:  Controller losgelassen, Antwort laeuft -> kleine Notch mit Animation in der Mitte
 * - off:   nichts (auch "ready": Verbindung steht noch, Notch darf wieder weg)
 */
function voiceMode(): "off" | "panel" | "mini" {
  const s = voiceUi;
  if (s === "off" || s === "ready") return "off";
  if (s === "error" || s === "nokey") return padVoiceErr && Date.now() >= padErrUntil ? "off" : "panel";
  if (!voice.isPtt) return "panel";
  if (voice.isHeld) return "panel";
  return s === "thinking" || s === "speaking" || s === "connecting" ? "mini" : "off";
}
/** Fehler kam vom Controller-Sprechen (verschwindet von selbst) */
let padVoiceErr = false;
const PAD_ERR_MS = 6000;

function showVoice(s: VoiceState | "nokey", detail = "") {
  voiceUi = s;
  if (s === "error" || s === "nokey") {
    padVoiceErr = padVoice || arming();
    if (padVoiceErr) {
      padErrUntil = Date.now() + PAD_ERR_MS;
      setTimeout(() => render(), PAD_ERR_MS + 30);
    }
  } else padVoiceErr = false;
  voiceEl.hidden = voiceMode() !== "panel";
  voiceEl.dataset.s = s;
  q(".v-state", voiceEl).textContent = V_LABEL[s] ?? "";
  if (detail || s === "error" || s === "nokey") {
    q(".v-text", voiceEl).textContent =
      s === "nokey" ? `Einmal eintragen, danach mit ${voiceKey || "dem Mikrofon"} starten.` : detail;
  }
  q(".v-setup", voiceEl).hidden = s !== "nokey";
  micBtn.classList.toggle("on", voice.active);
  if (s === "listening" || s === "connecting") { view = "home"; notch.dataset.view = "home"; }
  render();
}

const findShelf = (name: string) => {
  const n = name.toLowerCase().trim();
  return shelf.find((i) => i.name.toLowerCase() === n) ?? shelf.find((i) => i.name.toLowerCase().includes(n));
};

const voice = new Voice({
  onState: (s, detail) => {
    showVoice(s, detail);
    if (s === "off" || s === "error") padVoice = false;
  },
  onTranscript: (t) => { q(".v-text", voiceEl).textContent = t; render(false); },
  onLevel: (l) => orb.style.setProperty("--lvl", l.toFixed(3)),
  onOutLevel: (l) => vminiLevel(l),
  // Werkzeuge, mit denen der Assistent die Notch bedient — alles ueber die vorhandenen Befehle
  tools: {
    musik: async (a) => { await invoke("media_control", { action: String(a.aktion) }); return { ok: true }; },
    lautstaerke: async (a) => {
      const pct = Math.max(0, Math.min(100, Number(a.prozent) || 0));
      volume = await invoke<Volume>("volume_set", { level: pct / 100 });
      renderVolume();
      return { prozent: Math.round(volume.level * 100) };
    },
    timer_starten: async (a) => { await invoke("timer_start", { seconds: Math.max(1, Number(a.minuten) || 1) * 60 }); return { ok: true }; },
    timer_stoppen: async () => { await invoke("activity_action", { id: "notch:timer", action: "stop" }); return { ok: true }; },
    status: async () => ({
      uhrzeit: new Date().toLocaleTimeString("de-DE", { hour: "2-digit", minute: "2-digit" }),
      musik: media ? { titel: media.title, kuenstler: media.artist, quelle: appName(media.source), spielt: media.playing } : null,
      eintraege: acts.map((a) => ({ app: a.app, titel: a.title, wert: [a.value, a.unit].filter(Boolean).join(" "), info: a.subtitle ?? "" })),
      ablage: shelf.map((i) => i.name),
    }),
    datei_oeffnen: async (a) => {
      const it = findShelf(String(a.name ?? ""));
      if (!it) return { fehler: "nicht in der Ablage", ablage: shelf.map((i) => i.name) };
      await invoke("open", { target: it.path });
      return { geoeffnet: it.name };
    },
    sport: async () => ({
      spiele: sport.matches.map((m) => ({
        wettbewerb: m.league_name, heim: m.home.name, gast: m.away.name, lieblingsteam: m.fav,
        stand: m.state === "pre" ? null : `${m.home.score}:${m.away.score}`,
        status: m.state === "pre" ? `Anstoß ${kickoff(m)}` : m.clock,
        letzte_meldungen: m.events.slice(-4).map((e) => [e.minute, e.title, e.text].filter(Boolean).join(" ")),
      })),
      hinweis: settings.sport.on ? undefined : "Live-Sport ist in den Einstellungen aus",
    }),
    datei_konvertieren: async (a) => {
      const it = findShelf(String(a.name ?? ""));
      if (!it) return { fehler: "nicht in der Ablage", ablage: shelf.map((i) => i.name) };
      const ziel = String(a.ziel ?? "").toLowerCase().replace(/^\./, "").replace("jpeg", "jpg");
      const targets = await invoke<Target[]>("convert_targets", { path: it.path });
      if (!targets.some((t) => t.id === ziel)) return { fehler: `${it.name} geht nicht nach ${ziel}`, moeglich: targets.map((t) => t.id) };
      await invoke("convert", { path: it.path, target: ziel });
      return { gestartet: `${it.name} → ${ziel}` };
    },
  },
});

async function toggleVoice() {
  if (voice.active && voiceUi !== "ready") { voice.stop(); return; }
  // unsichtbar wartende Controller-Sitzung: beenden und als normales Gespraech neu starten
  if (voice.active) voice.stop();
  if (!(await invoke<boolean>("voice_ready"))) { showVoice("nokey"); return; }
  void voice.start();
}

// ---------- Hilfen ----------

const fmt = (s: number) => {
  s = Math.max(0, Math.floor(s));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = String(s % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
};

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
}

const ICONS: Record<string, string> = {
  play: "M8 5v14l11-7z",
  pause: "M7 5h4v14H7zM13 5h4v14h-4z",
  stop: "M6 6h12v12H6z",
  check: "M9.5 16.2 5.3 12l-1.4 1.4 5.6 5.6L20.5 8l-1.4-1.4z",
  close: "M6.4 5 12 10.6 17.6 5 19 6.4 13.4 12l5.6 5.6-1.4 1.4-5.6-5.6L6.4 19 5 17.6l5.6-5.6L5 6.4z",
  folder: "M3 6a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z",
  open: "M14 4h6v6h-2V7.4l-7.3 7.3-1.4-1.4L16.6 6H14zM5 6h6v2H6v10h10v-5h2v6a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1z",
  restart: "M12 5V2L7 6l5 4V7a5 5 0 1 1-5 5H5a7 7 0 1 0 7-7z",
  plus: "M11 5h2v6h6v2h-6v6h-2v-6H5v-2h6z",
  search: "M10.5 4a6.5 6.5 0 0 1 5.2 10.4l4.4 4.4-1.4 1.4-4.4-4.4A6.5 6.5 0 1 1 10.5 4zm0 2a4.5 4.5 0 1 0 0 9 4.5 4.5 0 0 0 0-9z",
  up: "M12 8.6 18.4 15 17 16.4l-5-5-5 5L5.6 15z",
  down: "M12 15.4 5.6 9 7 7.6l5 5 5-5L18.4 9z",
};

function svgIcon(name: string) {
  const d = ICONS[name];
  if (!d) return null;
  const s = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  s.setAttribute("viewBox", "0 0 24 24");
  const p = document.createElementNS("http://www.w3.org/2000/svg", "path");
  p.setAttribute("d", d);
  s.append(p);
  return s;
}

function iconEl(a: Activity) {
  const box = el("div", "icon");
  const i = a.icon ?? "";
  if (i.startsWith("data:") || i.startsWith("http")) {
    // Bilder (z. B. PDF-Deckel) im eigenen Seitenverhaeltnis zeigen, nicht im Kreis beschnitten
    if (!i.startsWith("data:image/svg")) box.classList.add("pic");
    const img = new Image();
    img.src = i;
    box.append(img);
  } else {
    box.textContent = i || (a.app || a.title).slice(0, 1).toUpperCase();
  }
  if (a.pulse && a.pulse > 0) {
    box.dataset.pulse = a.id;
    pulseCatchUp(box);
  }
  return box;
}

// ---------- Pulse-Taktgeber ----------
// Activities mit `pulse` (bpm) lassen ihr Symbol im Takt schlagen. Die Phase laeuft pro Activity
// weiter, auch wenn Zeilen jede Sekunde neu gebaut werden oder sich der Takt aendert: ein
// neu gebautes Symbol setzt einen laufenden Schlag an der richtigen Stelle fort.
const BEAT: Keyframe[] = [
  { transform: "scale(1)", offset: 0 },
  { transform: "scale(1.22)", offset: 0.14 },
  { transform: "scale(0.96)", offset: 0.3 },
  { transform: "scale(1.1)", offset: 0.44 },
  { transform: "scale(1)", offset: 0.72 },
  { transform: "scale(1)", offset: 1 },
];
const pulses = new Map<string, { phase: number; at: number; dur: number }>();
let pulseLast = 0;

function beatOn(e: HTMLElement, dur: number, from = 0) {
  // nur das Symbol schlaegt, der getoente Kreis dahinter bleibt ruhig
  const target = (e.querySelector("img") as HTMLElement | null) ?? e;
  const anim = target.animate(BEAT, { duration: dur, easing: "ease-out" });
  if (from > 0) anim.currentTime = from;
}

function pulseCatchUp(box: HTMLElement) {
  const p = pulses.get(box.dataset.pulse!);
  if (!p) return;
  const since = performance.now() - p.at;
  if (since < p.dur) beatOn(box, p.dur, since);
}

function pulseTick(t: number) {
  const dt = pulseLast ? Math.min((t - pulseLast) / 1000, 2) : 0;
  pulseLast = t;
  const live = new Set<string>();
  for (const a of acts) {
    if (!a.pulse || a.pulse <= 0) continue;
    live.add(a.id);
    let p = pulses.get(a.id);
    if (!p) { p = { phase: 0, at: 0, dur: 0 }; pulses.set(a.id, p); }
    p.phase += (dt * a.pulse) / 60;
    if (p.phase >= 1) {
      p.phase %= 1;
      p.at = t;
      p.dur = Math.min(560, (60_000 / a.pulse) * 0.62);
      if (!matchMedia("(prefers-reduced-motion: reduce)").matches) {
        for (const e of document.querySelectorAll<HTMLElement>(`[data-pulse="${CSS.escape(a.id)}"]`)) beatOn(e, p.dur);
      }
    }
  }
  for (const id of pulses.keys()) if (!live.has(id)) pulses.delete(id);
  requestAnimationFrame(pulseTick);
}
requestAnimationFrame(pulseTick);

function eq(playing: boolean) {
  const e = el("div", "eq" + (playing ? "" : " paused") + (Date.now() - meter.at < 400 ? " live" : ""));
  for (let i = 0; i < 4; i++) e.append(el("i"));
  return e;
}

/**
 * Echter Pegel statt Endlos-Animation: Rust misst ~30x/s den Peak der Audio-Sitzung des
 * Players. Jeder Balken bekommt eine eigene Verzoegerung, Gewichtung und Abklingzeit, damit
 * sie nicht im Gleichschritt huepfen. Automatische Aussteuerung, damit leise Stuecke nicht
 * platt und laute nicht dauernd am Anschlag sind. Kommt nichts mehr, faellt die CSS-Animation zurueck.
 */
const meter = { at: 0, ref: 0.2, hist: [] as number[], bars: [0.2, 0.2, 0.2, 0.2] };
const BAR_DELAY = [0, 2, 1, 3]; // in Messungen (~33 ms)
const BAR_GAIN = [0.75, 1, 0.9, 0.65];
const BAR_FALL = [0.8, 0.86, 0.83, 0.78];
let meterTimer = 0;

function onLevel(peak: number) {
  meter.at = Date.now();
  meter.ref = Math.max(peak, meter.ref * 0.997, 0.04);
  const n = Math.min(1, peak / meter.ref);
  meter.hist.unshift(n);
  meter.hist.length = Math.min(meter.hist.length, 8);
  const eqs = document.querySelectorAll<HTMLElement>(".eq");
  for (let i = 0; i < 4; i++) {
    const v = (meter.hist[BAR_DELAY[i]] ?? n) * BAR_GAIN[i];
    meter.bars[i] = Math.max(v, meter.bars[i] * BAR_FALL[i]);
  }
  eqs.forEach((e) => {
    e.classList.add("live");
    e.querySelectorAll<HTMLElement>("i").forEach((b, i) =>
      b.style.setProperty("--s", Math.max(0.18, meter.bars[i]).toFixed(3)),
    );
  });
  clearTimeout(meterTimer);
  meterTimer = window.setTimeout(() => document.querySelectorAll(".eq").forEach((e) => e.classList.remove("live")), 400);
}

/** Durchschnittsfarbe des Covers, etwas aufgehellt -> Akzent fuer Pegel und Schatten */
function accentFrom(src: string) {
  const img = new Image();
  img.onload = () => {
    const c = document.createElement("canvas");
    c.width = c.height = 8;
    const g = c.getContext("2d")!;
    g.drawImage(img, 0, 0, 8, 8);
    const d = g.getImageData(0, 0, 8, 8).data;
    let r = 0, gg = 0, b = 0, n = 0;
    for (let i = 0; i < d.length; i += 4) {
      const mx = Math.max(d[i], d[i + 1], d[i + 2]), mn = Math.min(d[i], d[i + 1], d[i + 2]);
      const w = 1 + (mx - mn) / 32; // bunte Pixel zaehlen mehr
      r += d[i] * w; gg += d[i + 1] * w; b += d[i + 2] * w; n += w;
    }
    const lift = (v: number) => Math.round(Math.min(255, 70 + (v / n) * 0.85));
    notch.style.setProperty("--accent", `rgb(${lift(r)}, ${lift(gg)}, ${lift(b)})`);
  };
  img.src = src;
}

// ---------- Schieberegler (Spulen + Lautstaerke) ----------

function setSlider(s: HTMLElement, frac: number) {
  const f = Math.min(1, Math.max(0, frac));
  q<HTMLElement>(".fill", s).style.width = `${f * 100}%`;
  q<HTMLElement>(".thumb", s).style.left = `${f * 100}%`;
}

/** Klicken oder Ziehen; onMove fuer die Vorschau, onEnd schickt den Wert ab. */
function makeSlider(s: HTMLElement, onMove: (f: number) => void, onEnd: (f: number) => void) {
  const frac = (e: PointerEvent) => {
    const r = s.getBoundingClientRect();
    return Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
  };
  s.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    s.setPointerCapture(e.pointerId);
    s.classList.add("drag");
    holding++;
    const f = frac(e);
    setSlider(s, f);
    onMove(f);
    const move = (ev: PointerEvent) => { const f = frac(ev); setSlider(s, f); onMove(f); };
    const up = (ev: PointerEvent) => {
      s.removeEventListener("pointermove", move);
      s.removeEventListener("pointerup", up);
      s.removeEventListener("pointercancel", up);
      s.classList.remove("drag");
      holding = Math.max(0, holding - 1);
      onEnd(frac(ev));
      if (!hoverRaw) setHover(false);
    };
    s.addEventListener("pointermove", move);
    s.addEventListener("pointerup", up);
    s.addEventListener("pointercancel", up);
  });
}

// ---------- Zustand ----------

function musicVisible() {
  if (!media) return false;
  return media.playing || Date.now() - pausedAt < pauseLinger();
}

// ---------- Kompakt: Rangliste (compact.ts) ----------

const pulseGate = new PulseGate();
/** Folio: zuletzt gemeldete Seite pro Eintrag; aendert sie sich, blendet die Seitenzahl kurz ein */
const folioPages = new Map<string, string | undefined>();
let folioFlash = { id: "", until: 0 };
let flashTimer = 0;

function watchFolio(list: Activity[]) {
  const seen = new Set<string>();
  for (const a of list) {
    if (sourceOf(a) !== "folio") continue;
    seen.add(a.id);
    const had = folioPages.has(a.id);
    const before = folioPages.get(a.id);
    folioPages.set(a.id, a.value);
    if (had && before !== a.value && settings.folio.flash) {
      const ms = settings.folio.flashSec * 1000;
      folioFlash = { id: a.id, until: Date.now() + ms };
      clearTimeout(flashTimer);
      flashTimer = window.setTimeout(() => render(), ms + 30);
    }
  }
  for (const id of [...folioPages.keys()]) if (!seen.has(id)) folioPages.delete(id);
}

function currentPlan(): Plan {
  return plan({
    acts,
    music: musicVisible(),
    s: settings,
    gate: pulseGate,
    folioFlash: Date.now() < folioFlash.until ? folioFlash.id : null,
  });
}

/** Timer abgelaufen und noch nicht bestaetigt? Dann bleibt die Notch offen und pulsiert. */
const timerAlarm = () => acts.some((a) => a.id === "notch:timer" && a.title.includes("abgelaufen"));

function wanted(): State {
  if (hover || dragOver || holding > 0 || typing || voiceMode() === "panel" || timerAlarm() || Date.now() < peekUntil || padMode === "expanded") return "expanded";
  // iPhone: Anfrage offen oder QR-Code sichtbar (nicht im Vollbild)
  if (!fullscreen && (shareAsk() || Date.now() < qrUntil)) return "expanded";
  // Sportmeldung: kurz aufklappen (nicht im Vollbild — da reicht der Spielstand in der kleinen Notch)
  if (Date.now() < newsUntil && !fullscreen) return "expanded";
  if (Date.now() < clipPeekUntil) return "clip";
  if (midKind()) return "compact";
  const p = currentPlan();
  if (p.slots.length || p.timer) return "compact";
  return "idle";
}

// ---------- Mitte der kleinen Notch: Ladering, Antwort-Animation, Spielstand ----------

const mid = q(".c-mid");
type MidKind = "" | "arm" | "voice" | "sport";

function midKind(): MidKind {
  if (arming()) return "arm";
  if (voiceMode() === "mini") return "voice";
  if (sportMid()) return "sport";
  return "";
}

/** Ladering: fuellt sich in der Restzeit bis zum Zuhoeren */
function armEl() {
  const box = el("div", "arm");
  box.style.setProperty("--arm", `${armMs}ms`);
  box.innerHTML =
    '<svg viewBox="0 0 24 24" class="arm-ring"><circle class="t" cx="12" cy="12" r="10"/><circle class="p" cx="12" cy="12" r="10"/></svg>' +
    '<svg viewBox="0 0 24 24" class="arm-mic"><path d="M12 3a3 3 0 0 1 3 3v6a3 3 0 0 1-6 0V6a3 3 0 0 1 3-3zm-7 9h2a5 5 0 0 0 10 0h2a7 7 0 0 1-6 6.9V21h-2v-2.1A7 7 0 0 1 5 12z"/></svg>';
  return box;
}

/** Antwort des Assistenten: fuenf Balken, die mit seiner Stimme gehen (beim Nachdenken eine ruhige Welle) */
function vminiEl() {
  const box = el("div", "vmini");
  box.dataset.s = voiceUi;
  for (let i = 0; i < 5; i++) box.append(el("i"));
  return box;
}
const VBAR = [0.55, 0.85, 1, 0.8, 0.5];
let vbarPhase = 0;
function vminiLevel(l: number) {
  const box = mid.querySelector<HTMLElement>(".vmini");
  if (!box) return;
  box.dataset.s = voiceUi;
  if (voiceUi !== "speaking") return;
  vbarPhase += 0.35;
  box.querySelectorAll<HTMLElement>("i").forEach((b, i) => {
    const wob = 0.75 + 0.25 * Math.sin(vbarPhase + i * 1.3);
    b.style.setProperty("--s", Math.max(0.16, Math.min(1, l * 1.6 * VBAR[i] * wob)).toFixed(3));
  });
}

function midSig() {
  const k = midKind();
  return k === "sport" ? "sport:" + sportMidSig() : k === "arm" ? `arm:${armUntil}` : k;
}

function fillMid() {
  const k = midKind();
  mid.dataset.kind = k;
  mid.replaceChildren();
  if (k === "arm") mid.append(armEl());
  else if (k === "voice") mid.append(vminiEl());
  else if (k === "sport") mid.append(sportMidEl());
}

// ---------- iPhone-Austausch (share.rs) ----------

type ShareDev = { fp: string; alias: string; model: string; recent: boolean };
type ShareState = { on: boolean; running: boolean; error: string; ip: string; port: number; alias: string; devices: ShareDev[] };
type ShareQr = { url: string; svg: string; minutes: number };
let shareState: ShareState | null = null;
/** QR-Code in der Ablage: Notch bleibt so lange offen (man greift ja zum iPhone) */
let qrUntil = 0;
const QR_MS = 120_000;
const qrPanel = q(".share-qr");
/** Anfrage vom iPhone wartet auf Annehmen/Ablehnen: Notch bleibt offen wie bei einem Anruf */
const shareAsk = () => acts.some((a) => a.id === "notch:share:ask");

function showQr(r: ShareQr, offered = 0) {
  q<HTMLImageElement>(".sq-img", qrPanel).src = `data:image/svg+xml;utf8,${encodeURIComponent(r.svg)}`;
  q(".sq-what", qrPanel).textContent = offered
    ? `${offered === 1 ? "1 Datei" : `${offered} Dateien`} zum Laden – oder Fotos hochladen`
    : "Öffnet eine Seite zum Hochladen auf diesen PC";
  q(".sq-alt", qrPanel).textContent = shareState?.alias ? `Mit LocalSend: „${shareState.alias}“ wählen` : "";
  q(".sq-url", qrPanel).textContent = `${r.url} · ${r.minutes} Min gültig`;
  qrPanel.classList.remove("err");
  qrPanel.hidden = false;
  qrUntil = Date.now() + QR_MS;
  setView("tray");
  setTimeout(() => { if (Date.now() >= qrUntil) closeQr(); }, QR_MS + 50);
}

function closeQr() {
  qrUntil = 0;
  qrPanel.hidden = true;
  if (!hoverRaw) setHover(false); else render();
}

/** Fehler (aus, kein WLAN, Geraet weg) kurz an derselben Stelle zeigen */
function shareError(msg: string) {
  q<HTMLImageElement>(".sq-img", qrPanel).removeAttribute("src");
  qrPanel.classList.add("err");
  q(".sq-what", qrPanel).textContent = msg;
  q(".sq-alt", qrPanel).textContent = "";
  q(".sq-url", qrPanel).textContent = "";
  qrPanel.hidden = false;
  qrUntil = Date.now() + 8000;
  setView("tray");
  setTimeout(() => { if (Date.now() >= qrUntil) closeQr(); }, 8050);
}

// ---------- Live-Sport (sport.rs / sport.ts) ----------

let sport: SportState = { matches: [] };
/** vom Nutzer gewaehltes Spiel (Klick auf ein anderes laufendes Spiel) */
let sportFocus = "";
/** letzte Meldung: ihr Spiel steht eine Minute lang im Mittelpunkt */
let hot: { n: SportNews; until: number } | null = null;
/** Meldung oben in der aufgeklappten Notch */
let news: SportNews | null = null;
let newsUntil = 0;
const NEWS_MS = 6500;
/** Vollbild („Am Rand“/„Nur anzeigen“): nach einem Tor kurz der Spielstand in der kleinen Notch */
let sportFlashUntil = 0;
const sportFlash = () => Date.now() < sportFlashUntil;
let rotate = 0;
const pitch = new Pitch();
const sportMatch = (key: string) => sport.matches.find((m) => m.key === key);
const hotNews = () => (hot && Date.now() < hot.until ? hot.n : null);

/** Kandidaten fuer die Mitte: laufende Spiele (Lieblingsteams zuerst), sonst Lieblingsteams kurz vor/nach dem Spiel */
function centerCandidates(): SportMatch[] {
  const sp = settings.sport;
  if (!sp.on || !sp.center) return [];
  const live = sport.matches.filter((m) => m.state === "in");
  const favLive = live.filter((m) => m.fav);
  if (favLive.length) return favLive;
  if (live.length) return live;
  const now = Date.now();
  return sport.matches.filter((m) => m.fav && (
    (m.state === "pre" && m.start - now < 60 * 60_000) || (m.state === "post" && now - m.start < 150 * 60_000)));
}

/** Spiel in der Mitte der kleinen Notch (mehrere laufende: alle 8 s das naechste, Konferenz) */
function sportMid(): SportMatch | null {
  const h = hotNews();
  const hm = h ? sportMatch(h.key) : undefined;
  if (hm && (sportFlash() || (settings.sport.center && Date.now() - h!.at < 60_000))) return hm;
  const c = centerCandidates();
  return c.length ? c[rotate % c.length] : null;
}
/** Tor-Leuchten nur in den ersten Sekunden nach der Meldung */
const glowNews = () => { const h = hotNews(); return h && Date.now() - h.at < 8000 ? h : null; };
const sportMidSig = () => { const m = sportMid(); return m ? sportSig(m, glowNews()) : ""; };
const sportMidEl = () => midEl(sportMid()!, glowNews());

/** Spiel fuer die Karte: gewaehlt > letzte Meldung > Lieblingsteam live > erstes laufendes; sonst nur Lieblingsteams in der Naehe */
function cardFocus(): SportMatch | undefined {
  if (!settings.sport.on || !sport.matches.length) return undefined;
  const pick = sportMatch(sportFocus) ?? (hotNews() ? sportMatch(hotNews()!.key) : undefined);
  if (pick) return pick;
  const live = sport.matches.filter((m) => m.state === "in");
  if (live.length) return live.find((m) => m.fav) ?? live[0];
  const now = Date.now();
  return sport.matches.find((m) => m.fav && Math.abs(m.start - now) < 3 * 3600_000);
}

const sportSlot = q(".sport-slot");
const bannerSlot = q(".sport-banner");
let sportCardSig = "";
let bannerSig = "";
/** Arena (eigene Sport-App) installiert? Alle 5 Min neu nachsehen */
let arena = false;
let arenaAt = 0;
function checkArena() {
  if (Date.now() - arenaAt < 5 * 60_000) return;
  arenaAt = Date.now();
  invoke<boolean>("arena_installed").then((v) => { if (v !== arena) { arena = v; render(); } }).catch(() => {});
}
const openArena = (key = "") => invoke("open", { target: key ? `arena://spiel/${encodeURIComponent(key)}` : "arena://start" }).catch(() => {});

function renderSport() {
  const f = cardFocus();
  const usePitch = !!f && f.pitch && f.source === "espn" && settings.sport.pitch && f.state !== "pre";
  const others = f ? sport.matches.filter((m) => m.key !== f.key && m.state === "in") : [];
  if (f) checkArena();
  const sig = f ? cardSig(f, others, dock !== "top", usePitch, arena) : "";
  if (sig !== sportCardSig) {
    sportCardSig = sig;
    if (f && usePitch) pitch.setMatch(f);
    sportSlot.replaceChildren(...(f ? [cardEl(f, others, {
      pitch: usePitch ? pitch : null,
      side: dock !== "top",
      onOpen: (m) => { if (m.link) invoke("open", { target: m.link }).catch(() => {}); },
      onArena: arena ? (m) => void openArena(m.key) : undefined,
      onFocus: (key) => { sportFocus = key; hot = null; render(); },
    })] : []));
  }
  sportSlot.hidden = !f;
  // Meldung oben (unter der Leiste), solange sie frisch ist
  const show = news && Date.now() < newsUntil ? news : null;
  const bsig = show ? show.ev.id + show.score : "";
  if (bsig !== bannerSig) {
    bannerSig = bsig;
    bannerSlot.replaceChildren(...(show ? [bannerEl(show, sportMatch(show.key))] : []));
  }
  bannerSlot.hidden = !show;
}

/** Bei welchen Meldungen die Notch von selbst aufklappt */
function expandFor(big: number) {
  switch (settings.sport.expand) {
    case "off": return false;
    case "goals": return big >= 3;
    case "important": return big >= 2;
    default: return big >= 1;
  }
}

function onSportNews(n: SportNews) {
  if (!settings.sport.on || !sportMatch(n.key)) return;
  const now = Date.now();
  if (n.ev.big >= 2) { hot = { n, until: now + 60_000 }; sportFocus = ""; }
  if (!expandFor(n.ev.big)) { render(); return; }
  if (fullscreen) {
    if (settings.sport.fullscreen && settings.fullscreen.mode !== "hide" && n.ev.big >= 3) {
      sportFlashUntil = now + 7000;
      setTimeout(() => render(), 7050);
    }
  } else {
    news = n;
    newsUntil = now + NEWS_MS;
    if (!hover && view !== "home") { view = "home"; notch.dataset.view = "home"; }
    setTimeout(() => render(), NEWS_MS + 50);
  }
  setTimeout(() => render(), 8050); // Tor-Leuchten in der Mitte wieder aus
  render();
}

/** Ballverlauf nur holen und zeichnen, solange die Karte mit Spielfeld zu sehen ist */
let watching = "";
let watchAt = 0;
function tickWatch() {
  const f = cardFocus();
  const visible = state === "expanded" && (view === "home" || dock !== "top") && !!f && pitch.el.isConnected &&
    settings.sport.pitch && f.state !== "pre";
  const key = visible ? f!.key : "";
  if (key !== watching || (key && Date.now() - watchAt > 8000)) {
    watching = key;
    watchAt = Date.now();
    invoke("sport_watch", { key: key || null }).catch(() => {});
  }
  pitch.run(!!key);
}

let compactSig = "";
let compactKey = "";
let actsSig = "";
let shelfSig = "";

/** Platzbedarf der kleinen Notch (waechst z. B. mit laufendem Timer) */
let compactSize = 0;
/** nur die Mitte belegt (Ladering, Antwort, Spielstand): Notch darf schmaler sein als sonst */
let midOnly = false;

function renderCompact() {
  const p = currentPlan();
  // nur neu bauen, wenn sich etwas Sichtbares aendert (sonst startet der Pegel jede Sekunde neu)
  const s = settings;
  const sig = planSig(p, [media?.playing, coverSrc?.length, s.glucose.delta, s.music.eqBesideCover, dock, midSig()]);
  if (sig === compactSig) return;
  compactSig = sig;
  // wechselt die Quelle auf einem Platz, blenden die Elemente weich ein (nicht bei jeder neuen Zahl)
  const key = planKey(p);
  const animate = compactKey !== "" && key !== compactKey;
  compactKey = key;
  build(lead, trail, p, s, { coverSrc, musicPlaying: !!media?.playing, iconEl: (a) => iconEl(a as Activity), eq }, animate);
  fillMid();
  const side = dock !== "top";
  midOnly = !!midKind() && !lead.childElementCount && !trail.childElementCount;
  const base = midOnly ? (side ? 90 : 120) : side ? SIDE.compact.h : SIZE.compact.w;
  const measure = () => { compactSize = needed(lead, trail, side, base, mid); };
  measure();
  // Cover/Deckel haben erst nach dem Laden eine Breite -> dann nachmessen
  for (const img of [...lead.querySelectorAll("img"), ...trail.querySelectorAll("img")]) {
    if (!img.complete) img.addEventListener("load", () => { measure(); render(false); }, { once: true });
  }
}

/**
 * Cover im echten Seitenverhaeltnis: quadratische Alben bleiben 72×72, ein 16:9-Video-Vorschaubild
 * wird flach statt seitlich abgeschnitten. Leichte Rundung statt der grossen, die Ecken frass.
 */
function fitCover() {
  const apply = () => {
    const ar = cover.naturalWidth && cover.naturalHeight ? cover.naturalWidth / cover.naturalHeight : 1;
    const box = q<HTMLElement>(".cover-btn", player);
    const base = parseFloat(getComputedStyle(box).getPropertyValue("--cover")) || 72;
    box.style.width = `${Math.round(ar >= 1 ? base : base * ar)}px`;
    box.style.height = `${Math.round(ar >= 1 ? base / ar : base)}px`;
  };
  if (cover.complete) apply(); else cover.addEventListener("load", apply, { once: true });
}

function renderPlayer() {
  const show = !!media;
  player.hidden = !show;
  idleInfo.hidden = show || acts.length > 0 || !sportSlot.hidden;
  if (!media) return;
  q(".title", player).textContent = media.title;
  const app = appName(media.source);
  q(".artist", player).textContent = media.artist || (media.title === app ? "bereit" : app);
  const src = q<HTMLButtonElement>(".src", player);
  src.textContent = app;
  src.classList.toggle("multi", media.sessions > 1);
  src.title = media.sessions > 1 ? "Andere Quelle zeigen" : "";
  player.classList.toggle("playing", media.playing);
  if (coverSrc) { if (cover.getAttribute("src") !== coverSrc) { cover.src = coverSrc; fitCover(); } }
  else cover.removeAttribute("src");
  q(".t-dur", player).textContent = media.duration ? fmt(media.duration) : "";
  q<HTMLButtonElement>('[data-act="next"]', player).disabled = !media.can_next;
  q<HTMLButtonElement>('[data-act="prev"]', player).disabled = !media.can_prev;
  seekEl.classList.toggle("off", !media.duration);
  tickProgress();
}

function renderVolume() {
  const lvl = volume.muted || volume.level < 0.01 ? 0 : volume.level < 0.5 ? 1 : 2;
  volEl.dataset.lvl = String(lvl);
  if (!volSlider.classList.contains("drag")) setSlider(volSlider, volume.muted ? 0 : volume.level);
  // Welcher Ausgang geregelt wird, steht nur im Tooltip (sichtbar war es zu viel)
  volSlider.title = volume.device ? `Lautstärke · ${volume.device}` : "Lautstärke";
}

/**
 * Countdown-Balken (Timer): Rust schickt mit jeder Meldung `ends_at`. Daraus rechnet das
 * Frontend den Balken in jedem Bildschirmbild neu aus — keine Sekundenschritte, keine
 * Uebergaenge, die beim Neuaufbau der Zeile kurz haengen.
 */
const countdowns = new Map<HTMLElement, { p0: number; t0: number; end: number }>();
let cdRaf = 0;

function setProgress(a: Activity, s: HTMLElement) {
  const p = Math.min(1, Math.max(0, a.progress ?? 0));
  if (a.ends_at && a.ends_at > a.updated && p > 0) {
    // Startwert sofort setzen; weiter rechnet erst das naechste Bild — dann haengt der
    // Balken schon im DOM (vorher wurde er als "nicht mehr da" aussortiert und blieb leer)
    s.style.transition = "none";
    const c = { p0: p, t0: a.updated, end: a.ends_at };
    s.style.width = `${countdownAt(c, Date.now()) * 100}%`;
    countdowns.set(s, c);
    cancelAnimationFrame(cdRaf);
    cdRaf = requestAnimationFrame(tickCountdowns);
  } else {
    countdowns.delete(s);
    s.style.width = `${p * 100}%`;
  }
}

const countdownAt = (c: { p0: number; t0: number; end: number }, now: number) =>
  Math.max(0, Math.min(1, (c.p0 * (c.end - now)) / (c.end - c.t0)));

function tickCountdowns() {
  cancelAnimationFrame(cdRaf);
  const now = Date.now();
  for (const [s, c] of countdowns) {
    if (!s.isConnected) { countdowns.delete(s); continue; }
    s.style.width = `${(countdownAt(c, now) * 100).toFixed(3)}%`;
  }
  // nur rechnen, solange man es sieht
  if (countdowns.size && state === "expanded") cdRaf = requestAnimationFrame(tickCountdowns);
}

// ---------- Activity-Zeilen: bleiben bestehen und werden nur aktualisiert ----------
// (sonst verliert ein Eingabefeld in der Zeile bei jeder Seitenzahl-Meldung den Fokus)

const rows = new Map<string, HTMLElement>();
const rowAct = new WeakMap<HTMLElement, Activity>();

/** Tastatur in der Notch an/aus: solange ein Feld den Fokus hat, gibt Rust das Fenster nicht ab. */
let typing = false;
function setTyping(on: boolean) {
  if (typing === on) return;
  typing = on;
  invoke("keyboard", { on }).catch(() => {});
  render(false);
}

function inputBox(id: string) {
  const box = el("div", "act-input");
  const icon = el("span", "ai-icon");
  icon.append(svgIcon("search")!);
  const inp = el("input") as HTMLInputElement;
  inp.type = "text";
  inp.spellcheck = false;
  inp.autocomplete = "off";
  const prev = el("button", "ai-btn ai-prev");
  prev.title = "Vorheriger Treffer (Umschalt+Enter)";
  prev.append(svgIcon("up")!);
  const next = el("button", "ai-btn ai-next");
  next.title = "Nächster Treffer (Enter)";
  next.append(svgIcon("down")!);
  box.append(icon, inp, prev, next);

  let t = 0;
  const send = (kind: "input" | "submit" | "submit-prev") =>
    invoke("activity_input", { id, kind, value: inp.value }).catch(() => {});
  const sync = () => box.classList.toggle("filled", inp.value.length > 0);
  // Enter: Treffer anspringen und die App nach vorn holen
  const submit = (prevHit: boolean) => {
    if (!inp.value) return;
    window.clearTimeout(t);
    send(prevHit ? "submit-prev" : "submit");
    inp.blur();
    invoke("activity_open", { id }).catch(() => {});
  };

  box.addEventListener("click", (e) => e.stopPropagation());
  box.addEventListener("contextmenu", (e) => e.stopPropagation());
  // vor dem Fokus melden, sonst holt sich das vorige Programm die Tastatur zurueck
  inp.addEventListener("pointerdown", () => setTyping(true));
  inp.addEventListener("focus", () => setTyping(true));
  inp.addEventListener("blur", () => setTyping(false));
  inp.addEventListener("input", () => {
    sync();
    window.clearTimeout(t);
    t = window.setTimeout(() => send("input"), 180);
  });
  inp.addEventListener("keydown", (e) => {
    if (e.key === "Enter") { e.preventDefault(); submit(e.shiftKey); }
    else if (e.key === "Escape") {
      e.preventDefault();
      inp.value = "";
      sync();
      send("input");
      inp.blur();
    }
  });
  prev.addEventListener("click", () => submit(true));
  next.addEventListener("click", () => submit(false));
  return box;
}

/** Breite des Graphen: Panel oben 520 bzw. seitlich 400, abzueglich Innenabstand */
const bgWidth = () => (dock === "top" ? 480 : 360);

function fillRow(row: HTMLElement, a: Activity) {
  rowAct.set(row, a);
  if (isBg(a)) {
    // Verlaufs-Karte (Haze): eigener Aufbau, wird nur neu gezeichnet, wenn sich etwas Sichtbares aendert
    row.className = "act bg";
    row.style.removeProperty("--accent");
    row.title = "";
    const rebuilt = fillCard(row, a, {
      width: bgWidth(),
      onDouble: () => invoke("activity_focus_app", { id: a.id }),
      // Haze: Doppelklick auf die Zahl blendet das Widget ein/aus (Haze liest das Ereignis aus GET /events)
      onValueDouble: a.app === "Haze" ? () => { invoke("activity_action", { id: a.id, action: "widget" }).catch(() => {}); } : undefined,
      onChange: () => { const cur = rowAct.get(row); if (cur) fillRow(row, cur); render(false); },
    });
    if (rebuilt) {
      const x = el("button", "act-x");
      x.title = "Entfernen";
      x.append(svgIcon("close")!);
      x.addEventListener("click", (e) => { e.stopPropagation(); invoke("dismiss_activity", { id: a.id }); });
      row.append(x);
    }
    return;
  }
  delete row.dataset.bgsig;
  row.className = "act" + (a.open ? " link" : "") + (a.input ? " has-input" : "");
  if (a.color) row.style.setProperty("--accent", a.color);
  else row.style.removeProperty("--accent");
  row.title = a.open ? "Öffnen" : "";

  // alles ausser dem Eingabefeld neu aufbauen
  let box = row.querySelector<HTMLElement>(".act-input");
  for (const c of [...row.children]) if (c !== box) c.remove();

  const text = el("div", "act-text");
  text.append(el("div", "act-title", a.title));
  const sub = a.subtitle || (a.app !== a.title ? a.app : "");
  if (sub) text.append(el("div", "act-sub", sub));
  const val = el("div", "act-val", a.value ?? "");
  if (a.unit && a.value) val.append(el("small", "", a.unit));

  const x = el("button", "act-x");
  x.title = "Entfernen";
  x.append(svgIcon("close")!);
  x.addEventListener("click", (e) => { e.stopPropagation(); invoke("dismiss_activity", { id: a.id }); });

  const parts: Element[] = [iconEl(a), text, val, x];
  if (a.progress != null) {
    const p = el("div", "act-prog" + (a.progress < 0 ? " busy" : ""));
    const s = el("span");
    p.append(s);
    setProgress(a, s);
    parts.push(p);
  }
  if (a.actions?.length) {
    const bar = el("div", "act-actions");
    for (const ac of a.actions) {
      const ic = ac.icon ? svgIcon(ac.icon) : null;
      const b = el("button", "pill" + (ic ? " ic" : ""));
      b.title = ac.label;
      if (ic) b.append(ic); else b.textContent = ac.label;
      b.addEventListener("click", (e) => { e.stopPropagation(); invoke("activity_action", { id: a.id, action: ac.id }); });
      bar.append(b);
    }
    parts.push(bar);
  }
  for (const p of parts) row.insertBefore(p, box);

  if (a.input) {
    if (!box) { box = inputBox(a.id); row.append(box); }
    const inp = box.querySelector("input")!;
    inp.placeholder = a.input.placeholder || "Suchen";
    // Text der App uebernehmen, solange man nicht selbst tippt
    if (document.activeElement !== inp && a.input.value != null && inp.value !== a.input.value) {
      inp.value = a.input.value;
      box.classList.toggle("filled", inp.value.length > 0);
    }
  } else if (box) {
    box.remove();
  }
}

function newRow(id: string) {
  const row = el("div", "act");
  row.dataset.id = id;
  row.addEventListener("click", () => {
    if (rowAct.get(row)?.open) invoke("activity_open", { id });
  });
  row.addEventListener("contextmenu", (e) => { e.preventDefault(); invoke("dismiss_activity", { id }); });
  return row;
}

function renderActs() {
  // nur eine Puls-Zeile: die wichtigste (Garmin über Haze hat Vorrang vor Helio, siehe priority)
  const firstPulse = acts.find((a) => sourceOf(a) === "pulse");
  const list = acts.filter((a) => sourceOf(a) !== "pulse" || a === firstPulse).slice(0, 4);
  const sig = JSON.stringify(list) + dock;
  if (sig === actsSig) return;
  actsSig = sig;
  const keep = new Set(list.map((a) => a.id));
  for (const [id, r] of rows) if (!keep.has(id)) { r.remove(); rows.delete(id); }
  list.forEach((a, i) => {
    let row = rows.get(a.id);
    if (!row) { row = newRow(a.id); rows.set(a.id, row); }
    fillRow(row, a);
    // nur verschieben, wenn die Reihenfolge wirklich anders ist (Verschieben nimmt den Fokus)
    if (actsEl.children[i] !== row) actsEl.insertBefore(row, actsEl.children[i] ?? null);
  });
}

function fileThumb(it: Item) {
  const t = el("div", `fthumb k-${it.kind}`);
  if (it.kind === "image" && !["heic", "avif", "tif", "tiff"].includes(it.ext)) {
    const img = new Image();
    img.loading = "lazy";
    img.decoding = "async";
    img.src = convertFileSrc(it.path);
    img.onerror = () => { img.remove(); t.textContent = it.ext.toUpperCase().slice(0, 4); };
    t.append(img);
  } else {
    t.textContent = it.kind === "folder" ? "DIR" : (it.ext || "?").toUpperCase().slice(0, 4);
  }
  return t;
}

// ---------- Ablage: Auswahl (Strg/Umschalt), Doppelklick, Rechtsklick-Menue ----------

/** Pfade der markierten Dateien in Ablage-Reihenfolge */
const selectedItems = () => shelf.filter((i) => sel.has(i.path));

function selectClick(path: string, e: MouseEvent) {
  const order = shelf.map((i) => i.path);
  if (e.shiftKey && anchor && order.includes(anchor)) {
    const [a, b] = [order.indexOf(anchor), order.indexOf(path)].sort((x, y) => x - y);
    if (!e.ctrlKey) sel.clear();
    order.slice(a, b + 1).forEach((p) => sel.add(p));
  } else if (e.ctrlKey) {
    if (sel.has(path)) sel.delete(path); else sel.add(path);
    anchor = path;
  } else {
    const only = sel.size === 1 && sel.has(path);
    sel.clear();
    if (!only) sel.add(path);
    anchor = path;
  }
  applySel();
}

/** Auswahl nur per Klasse umschalten — kein Neuaufbau, sonst geht der Doppelklick verloren */
function applySel() {
  filesEl.querySelectorAll<HTMLElement>(".file").forEach((f) => f.classList.toggle("sel", sel.has(f.dataset.path!)));
}

function renderShelf() {
  badge.hidden = shelf.length === 0;
  badge.textContent = String(shelf.length);
  trayView.classList.toggle("has-files", shelf.length > 0);
  const known = new Set(shelf.map((i) => i.path));
  for (const p of [...sel]) if (!known.has(p)) sel.delete(p);

  const sig = JSON.stringify(shelf.map((i) => i.path));
  if (sig === shelfSig) return;
  shelfSig = sig;
  filesEl.replaceChildren(
    ...shelf.map((it) => {
      const f = el("div", "file" + (sel.has(it.path) ? " sel" : ""));
      f.dataset.path = it.path;
      f.title = `${it.path}\nDoppelklick: öffnen · Rechtsklick: umwandeln & mehr`;
      f.append(fileThumb(it), el("div", "fname", it.name));
      f.addEventListener("click", (e) => selectClick(it.path, e));
      f.addEventListener("dblclick", () => { closeMenu(); invoke("open", { target: it.path }); });
      f.addEventListener("contextmenu", (e) => {
        e.preventDefault();
        e.stopPropagation();
        if (!sel.has(it.path)) { sel.clear(); sel.add(it.path); anchor = it.path; applySel(); }
        void openMenu(e.clientX, e.clientY);
      });
      // Rausziehen: ab 6 px Bewegung uebernimmt Windows das Ziehen (alle markierten, wenn diese dabei ist)
      f.addEventListener("pointerdown", (e) => {
        if (e.button !== 0) return;
        const x0 = e.clientX, y0 = e.clientY;
        const mv = (ev: PointerEvent) => {
          if (Math.hypot(ev.clientX - x0, ev.clientY - y0) < 6) return;
          window.removeEventListener("pointermove", mv);
          const items = sel.has(it.path) ? selectedItems().map((i) => i.path) : [it.path];
          if (dragIcon) startDrag({ item: items, icon: dragIcon }).catch(() => {});
        };
        window.addEventListener("pointermove", mv);
        window.addEventListener("pointerup", () => window.removeEventListener("pointermove", mv), { once: true });
      });
      return f;
    }),
  );
}

// ---------- Zwischenablage: die letzten 5 Dinge ----------

const clipsWrap = q(".clips-wrap");
const clipsEl = q(".clips");
let clipsSig = "";
let copied = { id: 0, until: 0 };

// ---------- Neu kopiert: Notch faehrt kurz auf, das Ding faellt in die Ablage ----------

const CLIP_PEEK_MS = 2300;
let clipPeekUntil = 0;
let fresh = { id: 0, until: 0 };
let clipsReadyAt = Infinity; // erst nach dem Start reagieren (beim Start kommt der aktuelle Inhalt rein)
const seenClips = new Set<number>();
const peekEl = q(".clip-peek");

function clipLabel(c: Clip) {
  if (c.kind === "image") return `Bild · ${c.width} × ${c.height}`;
  if (c.kind === "files") return baseName(c.files[0] ?? "") + (c.files.length > 1 ? ` + ${c.files.length - 1} weitere` : "");
  return (c.text ?? "").replace(/\s+/g, " ").trim();
}

function onClips(list: Clip[]) {
  const top = list[0];
  // neu = noch nie gesehene id (erneutes Kopieren aus dem Verlauf schiebt nur nach oben -> kein Auftritt)
  const isNew = !!top && !seenClips.has(top.id) && Date.now() > clipsReadyAt;
  list.forEach((c) => seenClips.add(c.id));
  clips = list;
  if (isNew && !fullscreen) {
    fresh = { id: top.id, until: Date.now() + 1500 };
    if (state === "expanded") {
      // schon offen: nur Liste und Reiter reagieren lassen
      const tab = q('.tab[data-view="tray"]');
      tab.classList.remove("ping"); void tab.offsetWidth; tab.classList.add("ping");
    } else {
      const item = q(".cp-item", peekEl);
      item.className = `cp-item k-${top.kind}`;
      item.replaceChildren();
      if (top.kind === "image" && top.path) {
        const img = new Image();
        img.src = convertFileSrc(top.path);
        item.append(img);
      } else if (top.kind === "files") {
        const n = top.files.length;
        const ext = (top.files[0]?.split(".").pop() ?? "").toUpperCase();
        item.textContent = n > 1 ? String(n) : ext.length <= 4 ? ext : "DIR";
      } else {
        item.textContent = "T";
      }
      q(".cp-text", peekEl).textContent = clipLabel(top);
      peekEl.classList.remove("go"); void peekEl.offsetWidth; peekEl.classList.add("go");
      clipPeekUntil = Date.now() + CLIP_PEEK_MS;
      setTimeout(() => { if (Date.now() >= clipPeekUntil) { peekEl.classList.remove("go"); render(false); } }, CLIP_PEEK_MS + 30);
    }
  }
  render();
}

function ago(at: number) {
  const s = Math.max(0, (Date.now() - at) / 1000);
  if (s < 60) return "jetzt";
  if (s < 3600) return `${Math.floor(s / 60)} min`;
  if (s < 86400) return `${Math.floor(s / 3600)} h`;
  return `${Math.floor(s / 86400)} T`;
}

const baseName = (p: string) => p.replace(/[\\/]+$/, "").split(/[\\/]/).pop() ?? p;

function clipRow(c: Clip) {
  const row = el("div", `clip k-${c.kind}` + (fresh.id === c.id && Date.now() < fresh.until ? " fresh" : ""));
  const lead = el("div", "clip-lead");
  const label = el("div", "clip-text");
  if (c.kind === "image" && c.path) {
    const img = new Image();
    img.decoding = "async";
    img.src = convertFileSrc(c.path);
    lead.append(img);
    label.textContent = `Bild · ${c.width} × ${c.height}`;
    row.title = "Klick: wieder kopieren · Ziehen: als Datei ablegen";
  } else if (c.kind === "files") {
    const first = c.files[0] ?? "";
    const ext = (first.split(".").pop() ?? "").toUpperCase();
    lead.textContent = c.files.length > 1 ? String(c.files.length) : ext.length <= 4 && first.includes(".") ? ext : "DIR";
    label.textContent = baseName(first) + (c.files.length > 1 ? ` + ${c.files.length - 1} weitere` : "");
    row.title = c.files.join("\n") + "\n\nKlick: wieder kopieren · Ziehen: Dateien ablegen";
  } else {
    lead.textContent = "T";
    label.textContent = (c.text ?? "").replace(/\s+/g, " ").trim();
    row.title = (c.text ?? "").slice(0, 600) + "\n\nKlick: wieder kopieren";
  }
  const age = el("div", "clip-age", ago(c.at));
  age.dataset.at = String(c.at);
  const done = el("div", "clip-done", "Kopiert");
  row.append(lead, label, age, done);

  if (c.kind !== "text") {
    const toShelf = el("button", "clip-btn");
    toShelf.title = "In die Ablage legen";
    toShelf.append(svgIcon("plus")!);
    toShelf.addEventListener("click", (e) => { e.stopPropagation(); invoke("clip_to_shelf", { id: c.id }); });
    row.append(toShelf);
  }
  const x = el("button", "clip-btn");
  x.title = "Aus dem Verlauf nehmen";
  x.append(svgIcon("close")!);
  x.addEventListener("click", (e) => { e.stopPropagation(); invoke("clip_remove", { id: c.id }); });
  row.append(x);

  // "Kopiert" ueberlebt den Neuaufbau (der Eintrag rutscht beim Kopieren nach oben)
  const flash = () => {
    row.classList.add("copied");
    setTimeout(() => row.classList.remove("copied"), Math.max(0, copied.until - Date.now()));
  };
  if (copied.id === c.id && Date.now() < copied.until) flash();
  row.addEventListener("click", () => {
    copied = { id: c.id, until: Date.now() + 1100 };
    flash();
    invoke("clip_copy", { id: c.id }).catch(() => { row.classList.remove("copied"); });
  });
  // Bilder und Dateien lassen sich direkt rausziehen
  const paths = c.kind === "files" ? c.files : c.kind === "image" && c.path ? [c.path] : [];
  if (paths.length) {
    row.addEventListener("pointerdown", (e) => {
      if (e.button !== 0 || (e.target as Element).closest("button")) return;
      const x0 = e.clientX, y0 = e.clientY;
      const mv = (ev: PointerEvent) => {
        if (Math.hypot(ev.clientX - x0, ev.clientY - y0) < 6) return;
        window.removeEventListener("pointermove", mv);
        if (dragIcon) startDrag({ item: paths, icon: dragIcon }).catch(() => {});
      };
      window.addEventListener("pointermove", mv);
      window.addEventListener("pointerup", () => window.removeEventListener("pointermove", mv), { once: true });
    });
  }
  return row;
}

function renderClips() {
  clipsWrap.hidden = clips.length === 0;
  const sig = JSON.stringify(clips.map((c) => [c.id, c.at]));
  if (sig === clipsSig) return;
  clipsSig = sig;
  clipsEl.replaceChildren(...clips.map(clipRow));
}

function tickAges() {
  clipsEl.querySelectorAll<HTMLElement>(".clip-age").forEach((a) => { a.textContent = ago(Number(a.dataset.at)); });
}

/** Ziele, die fuer ALLE markierten Dateien gehen */
async function commonTargets(items: Item[]) {
  let common: Target[] | null = null;
  for (const it of items) {
    const t = await invoke<Target[]>("convert_targets", { path: it.path });
    common = common ? common.filter((c) => t.some((x) => x.id === c.id)) : t;
  }
  return common ?? [];
}

function convertSelected(target: Target) {
  const items = selectedItems();
  if (!items.length) return;
  conv = {
    total: items.length,
    since: Date.now() - 50,
    label: `${items.length > 1 ? items.length + " Dateien" : items[0].name} → ${target.label}`,
    done: new Map(),
    failed: new Map(),
    hideAt: 0,
  };
  for (const it of items) invoke("convert", { path: it.path, target: target.id });
  closeMenu();
  renderFileBar(); // in der Ablage bleiben; Fortschritt unten, Ergebnis landet in der Ablage
}

const ACTIONS = {
  open: () => selectedItems().forEach((i) => invoke("open", { target: i.path })),
  reveal: () => { const i = selectedItems()[0]; if (i) invoke("reveal", { path: i.path }); },
  remove: () => { selectedItems().forEach((i) => invoke("shelf_remove", { path: i.path })); sel.clear(); },
};

/** Unten in der Ablage: nur noch der Stand der Umwandlung (alle Aktionen sind im Rechtsklick-Menue). */
function renderFileBar() {
  if (conv?.hideAt && Date.now() > conv.hideAt) conv = null;
  fileBar.hidden = !conv;
  if (!conv) { render(false); return; }
  // eigene Umwandlungen: notch:convert:*-Eintraege, die nach dem Start entstanden sind
  for (const a of acts) {
    if (!a.id.startsWith("notch:convert:") || a.updated < conv.since) continue;
    if (a.subtitle === "fertig") conv.done.set(a.id, a.title);
    else if (a.color === "#ff453a") conv.failed.set(a.id, a.subtitle ?? "Fehler");
  }
  const n = conv.done.size + conv.failed.size;
  const finished = n >= conv.total;
  const prog = q(".fb-prog", fileBar);
  const bar = q<HTMLElement>("span", prog);
  prog.classList.toggle("busy", !finished && n === 0);
  prog.classList.toggle("ok", finished && !conv.failed.size);
  prog.classList.toggle("err", finished && conv.failed.size > 0);
  bar.style.width = `${Math.round((finished ? 1 : n / conv.total) * 100)}%`;
  let text = conv.label + (conv.total > 1 ? ` — ${n}/${conv.total}` : " …");
  if (finished) {
    const [err] = conv.failed.values();
    text = conv.failed.size
      ? `Fehler: ${err}`
      : conv.total > 1 ? `Fertig: ${conv.total} Dateien — liegen neben den Originalen` : `Fertig: ${[...conv.done.values()][0]}`;
    if (!conv.hideAt) {
      const ms = conv.failed.size ? 9000 : 5000;
      conv.hideAt = Date.now() + ms;
      setTimeout(renderFileBar, ms + 50);
    }
  }
  q(".fb-name", fileBar).textContent = text;
  render(false);
}

// Rechtsklick-Menue: liegt in der Notch, damit es nie aus dem Fenster ragt
const menu = el("div", "ctx");
menu.hidden = true;
let menuOpen = false;

async function openMenu(x: number, y: number) {
  const items = selectedItems();
  if (!items.length) return;
  const n = items.length;
  menu.replaceChildren();
  const add = (label: string, fn: () => void, cls = "") => {
    const b = el("button", "ctx-item " + cls, label);
    b.addEventListener("click", () => { fn(); closeMenu(); });
    menu.append(b);
  };
  add(n > 1 ? `${n} Dateien öffnen` : "Öffnen", ACTIONS.open);
  add("Im Ordner zeigen", ACTIONS.reveal);
  // iPhone: per LocalSend an ein Geraet in der Naehe, sonst per QR-Code im Browser
  menu.append(el("div", "ctx-sep"));
  menu.append(el("div", "ctx-label", "An iPhone senden"));
  if (!shareState?.on) {
    add("iPhone-Austausch einschalten …", () => { invoke("open_settings", { tab: "share" }).catch(() => {}); });
  } else {
    const files = items.filter((i) => i.kind !== "folder").map((i) => i.path);
    for (const d of (shareState.devices ?? []).filter((x) => x.recent)) {
      add(`📱 ${d.alias}`, () => { invoke("share_send", { fp: d.fp, paths: files }).catch((e) => shareError(String(e))); });
    }
    add("Per QR-Code (Browser) …", () => {
      invoke<ShareQr>("share_offer", { paths: files }).then((r) => showQr(r, files.length)).catch((e) => shareError(String(e)));
    });
  }
  const targets = await commonTargets(items);
  if (targets.length) {
    menu.append(el("div", "ctx-sep"));
    menu.append(el("div", "ctx-label", "Umwandeln in"));
    const row = el("div", "ctx-formats");
    for (const t of targets) {
      const b = el("button", "pill", t.label);
      b.addEventListener("click", () => convertSelected(t));
      row.append(b);
    }
    menu.append(row);
  } else {
    const av = items.some((i) => i.kind === "audio" || i.kind === "video");
    const hint = av && !(await invoke<boolean>("ffmpeg_available")) ? "Für Audio/Video fehlt ffmpeg"
      : n > 1 ? "Kein gemeinsames Zielformat" : "";
    if (hint) { menu.append(el("div", "ctx-sep")); menu.append(el("div", "ctx-label", hint)); }
  }
  menu.append(el("div", "ctx-sep"));
  add(n > 1 ? `${n} aus der Ablage nehmen` : "Aus der Ablage nehmen", ACTIONS.remove, "danger");

  // Position relativ zur Form; an den Raendern nach innen schieben
  const host = q(".shape").getBoundingClientRect();
  menu.hidden = false;
  menuOpen = true;
  const mw = menu.offsetWidth, mh = menu.offsetHeight;
  const left = Math.min(Math.max(8, x - host.left), host.width - mw - 8);
  // Nach unten darf das Menue ueberstehen: die Notch waechst dann mit (siehe render)
  let top = y - host.top;
  if (top + mh > MAX_H - 8) top = Math.max(8, MAX_H - 8 - mh);
  menu.style.left = `${left}px`;
  menu.style.top = `${top}px`;
  render(false);
}

function closeMenu() {
  if (!menuOpen) return;
  menuOpen = false;
  menu.hidden = true;
  render(false);
}

/** Timer einstellen wie auf dem iPhone: drei Drehraeder (Std./Min./Sek.) und ein runder Start-Knopf.
 *  Die letzte Einstellung bleibt gemerkt. */
let pickWheels: Wheel[] = [];
function renderPresets() {
  const box = q(".timer-pick");
  if (box.childElementCount) return;
  let last = [0, 5, 0];
  try {
    const s = JSON.parse(localStorage.getItem("timer-pick") ?? "null");
    if (Array.isArray(s) && s.length === 3) last = s.map((n) => Number(n) || 0);
  } catch { /* kein Speicher, egal */ }
  const hold = (on: boolean) => { holding = Math.max(0, holding + (on ? 1 : -1)); if (!on && !hoverRaw) setHover(false); };
  pickWheels = [new Wheel(24, last[0], "Std.", hold), new Wheel(60, last[1], "Min.", hold), new Wheel(60, last[2], "Sek.", hold)];
  const picker = el("div", "picker");
  picker.append(el("div", "pick-band"), ...pickWheels.map((w) => w.el));
  const start = el("button", "pick-start", "Start");
  start.addEventListener("click", () => {
    const [h, m, s] = pickWheels.map((w) => w.get());
    const secs = h * 3600 + m * 60 + s;
    if (!secs) { start.classList.remove("nope"); void start.offsetWidth; start.classList.add("nope"); return; }
    try { localStorage.setItem("timer-pick", JSON.stringify([h, m, s])); } catch { /* egal */ }
    invoke("timer_start", { seconds: secs });
    setView("home");
  });
  box.append(picker, start);
}

function tickProgress() {
  if (!media || seekEl.classList.contains("drag")) return;
  let pos = media.position;
  if (media.playing) pos += (Date.now() - mediaAt) / 1000;
  if (media.duration) pos = Math.min(pos, media.duration);
  setSlider(seekEl, media.duration ? pos / media.duration : 0);
  q(".t-pos", player).textContent = media.duration ? fmt(pos) : "live";
}

function tickClock() {
  const d = new Date();
  const t = d.toLocaleTimeString("de-DE", { hour: "2-digit", minute: "2-digit" });
  q(".clock").textContent = t;
  q(".big-clock").textContent = t;
  q(".date").textContent = d.toLocaleDateString("de-DE", { weekday: "long", day: "numeric", month: "long" });
}

function setView(v: View) {
  view = v;
  notch.dataset.view = v;
  if (v === "timer") renderPresets();
  render();
}

function render(full = true) {
  // kleine Notch immer pruefen (baut nur bei Aenderung neu): die Mitte wechselt auch ohne neue Daten
  renderCompact();
  renderSport();
  if (full) {
    renderPlayer();
    renderActs();
    renderShelf();
    renderClips();
    renderVolume();
    tickClock();
  }

  state = wanted();
  notch.dataset.state = state;
  notch.classList.toggle("gone", hiddenByFullscreen());
  notch.classList.toggle("dragging", dragOver);
  notch.classList.toggle("alarm", timerAlarm());
  if (timerAlarm() && view !== "home") { view = "home"; notch.dataset.view = "home"; }

  notch.dataset.dock = dock;
  const side = dock !== "top";
  const s = { ...(side ? SIDE : SIZE)[state] };
  // kompakt: so breit (seitlich: so hoch) wie der Inhalt braucht, mindestens die Grundgroesse
  // (nur die Mitte belegt: so klein wie die Mitte es zulaesst)
  if (state === "compact" && compactSize) {
    if (side) s.h = Math.min(Math.floor(innerHeight * 0.8), midOnly ? compactSize : Math.max(s.h, compactSize));
    else s.w = Math.min(innerWidth - 2 * EAR - 8, midOnly ? compactSize : Math.max(s.w, compactSize));
  }
  if (state === "expanded") {
    expanded.style.setProperty("--xw", `${s.w}px`);
    const max = side ? Math.floor(innerHeight * 0.92) : MAX_H;
    s.h = Math.min(max, Math.ceil(expanded.scrollHeight));
    // offenes Rechtsklick-Menue: Notch waechst mit, statt es abzuschneiden
    if (menuOpen) s.h = Math.min(max, Math.max(s.h, menu.offsetTop + menu.offsetHeight + 12));
  }
  const e = state === "expanded" ? EAR_X : EAR;
  notch.style.setProperty("--w", `${s.w}px`);
  notch.style.setProperty("--h", `${s.h}px`);
  notch.style.setProperty("--r", `${s.r}px`);
  notch.style.setProperty("--e", `${e}px`);
  clearTray.hidden = !((side || view === "tray") && shelf.length > 0);
  shareBtn.hidden = !(side || view === "tray");

  // Wo die Form im Fenster liegt -> nur dort faengt das Fenster die Maus
  let rect: Rect =
    dock === "top"
      ? { x: (innerWidth - s.w) / 2 - e, y: 0, w: s.w + 2 * e, h: s.h }
      : { x: dock === "left" ? 0 : innerWidth - s.w, y: (innerHeight - s.h) / 2 - e, w: s.w, h: s.h + 2 * e };
  // Schrumpft der Inhalt unter der Maus (z. B. Leiste verschwindet nach einem Klick), bleibt der
  // Fangbereich so gross wie vorher, bis die Maus wirklich weg ist — sonst klappt die Notch einem
  // unter dem Zeiger weg.
  if (state === "expanded" && hoverRaw && lastRect) rect = union(rect, lastRect);
  lastRect = state === "expanded" && hoverRaw ? rect : null;
  invoke("set_hit_rect", { rect: passiveInFullscreen() ? { x: 0, y: 0, w: 0, h: 0 } : rect });
  if (state === "expanded" && countdowns.size) tickCountdowns();
}

type Rect = { x: number; y: number; w: number; h: number };
let lastRect: Rect | null = null;
function union(a: Rect, b: Rect): Rect {
  const x = Math.min(a.x, b.x), y = Math.min(a.y, b.y);
  return { x, y, w: Math.max(a.x + a.w, b.x + b.w) - x, h: Math.max(a.y + a.h, b.y + b.h) - y };
}

function setHover(v: boolean) {
  const was = state === "expanded";
  hover = v;
  if (v && !was) {
    // Aufklappen: Lautstaerke frisch holen; Timer-Auswahl nicht wieder anzeigen
    if (view === "timer") { view = "home"; notch.dataset.view = "home"; }
    invoke<Volume>("volume_get").then((v) => { volume = v; renderVolume(); }).catch(() => {});
  }
  if (!v) {
    // Maus ist weg: Menue zu; Auswahl bleibt, damit man nach dem Zurueckkommen weitermachen kann
    closeMenu();
    // Suchfeld: ist es leer, Tastatur sofort zurueckgeben; mit Text bleibt man drin,
    // bis man woanders klickt (dann holt sich Windows den Fokus ohnehin)
    const ae = document.activeElement as HTMLInputElement | null;
    if (ae?.tagName === "INPUT" && !ae.value) ae.blur();
  }
  render();
}

// ---------- Events ----------

function setMedia(m: Media) {
  if (media?.playing && m && !m.playing) pausedAt = Date.now();
  media = m;
  mediaAt = Date.now();
  if (!m) coverSrc = null;
}

function setCover(src: string | null) {
  coverSrc = src;
  if (src) accentFrom(src);
  else notch.style.removeProperty("--accent");
}

async function main() {
  // Liquid Glass (gemeinsame Designsprache): Sportmeldung, Spielfeld-Beschriftung, QR-Code
  liquid();
  // Erst zuhoeren, dann den Stand holen — sonst gehen Ereignisse beim Start verloren.
  await listen<Media>("media", (e) => { setMedia(e.payload); render(); });
  await listen<{ key: string; src: string }>("media-cover", (e) => { setCover(e.payload.src); render(); });
  await listen<Activity[]>("activities", (e) => {
    acts = e.payload;
    watchFolio(acts);
    // Stand der Umwandlung in der Ablage-Leiste zeigen (man bleibt in der Ablage)
    if (conv) renderFileBar();
    render();
  });
  await listen<Item[]>("shelf", (e) => { shelf = e.payload; render(); });
  await listen<number>("level", (e) => onLevel(e.payload));
  await listen<Clip[]>("clips", (e) => onClips(e.payload));
  await listen<string>("activity-alert", () => {
    peekUntil = Date.now() + 3500;
    render();
    setTimeout(render, 3600);
  });
  await listen<boolean>("fullscreen", (e) => { fullscreen = e.payload; render(); });
  await listen<boolean>("peek", (e) => { peek = e.payload; render(); });
  await listen<boolean>("hover", (e) => {
    hoverRaw = e.payload;
    clearTimeout(hoverTimer);
    if (!e.payload && holding > 0) return; // waehrend des Ziehens offen bleiben
    // kurz verweilen lassen, damit ein Griff nach der Titelleiste die Notch nicht aufreisst
    hoverTimer = window.setTimeout(() => setHover(e.payload), e.payload ? 110 : 280);
  });

  // Dateien von aussen reinziehen
  await getCurrentWebview().onDragDropEvent((e) => {
    const p = e.payload;
    if (p.type === "enter") {
      dragOver = true;
      if (view !== "tray") { view = "tray"; notch.dataset.view = "tray"; }
    } else if (p.type === "drop") {
      dragOver = false;
      if (p.paths?.length) invoke("shelf_add", { paths: p.paths });
    } else if (p.type === "leave") {
      dragOver = false;
    }
    if (p.type !== "over") render();
  });

  // Andocken: oben / links / rechts
  await listen<Dock>("dock", (e) => {
    dock = e.payload;
    render();
    setTimeout(render, 80); // Fenstergroesse kommt einen Moment spaeter an
  });
  const hint = q(".dock-hint");
  await listen<Dock | null>("dock-preview", (e) => {
    notch.classList.toggle("moving", !!e.payload);
    hint.textContent = e.payload ? { top: "↑  Oben", left: "←  Links", right: "Rechts  →" }[e.payload] : "";
  });
  window.addEventListener("resize", () => render(false));

  // Musik-Reaktion: Rust schickt nur, wenn sie an ist, Musik laeuft und kein Vollbild vorne ist
  const reactor = new MusicReactor(
    q<HTMLCanvasElement>("canvas.react"),
    q(".shape"),
    () => settings.music.react,
    () => settings.music.layers,
    () => settings.music.strength,
    () => state === "expanded",
    () => dock,
  );
  // Einstellungen (Rangliste der kleinen Notch) — aendern sich live aus dem Einstellungsfenster
  await listen<unknown>("settings", (e) => {
    settings = normalize(e.payload);
    compactSig = "";
    if (settings.music.react === "off") reactor.stop();
    render();
  });
  settings = normalize(await invoke<unknown>("settings_get").catch(() => null));
  await listen<Spectrum>("spectrum", (e) => { if (!hiddenByFullscreen()) reactor.feed(e.payload); });
  q(".settings-btn").addEventListener("click", () => { invoke("open_settings").catch(() => {}); });
  // iPhone: Knopf in der Ablage zeigt den QR-Code (aus: Einstellungen oeffnen)
  shareState = await invoke<ShareState>("share_status").catch(() => null);
  await listen<ShareState>("share-state", (e) => { shareState = e.payload; render(false); });
  shareBtn.addEventListener("click", () => {
    if (!qrPanel.hidden) { closeQr(); return; }
    if (!shareState?.on) { invoke("open_settings", { tab: "share" }).catch(() => {}); return; }
    invoke<ShareQr>("share_qr").then((r) => showQr(r)).catch((e) => shareError(String(e)));
  });
  q(".sq-x", qrPanel).addEventListener("click", (e) => { e.stopPropagation(); closeQr(); });
  // Neue Dateien vom iPhone: aufgeklappt gleich die Ablage zeigen
  await listen<{ files: string[] }>("share-received", (e) => {
    if (e.payload.files.length && state === "expanded" && view !== "tray") setView("tray");
    if (!qrPanel.hidden) closeQr();
  });

  const snap = await invoke<{
    media: Media; cover: string | null; activities: Activity[]; shelf: Item[]; clips: Clip[]; fullscreen: boolean; peek?: boolean; hover: boolean;
    dock: Dock; voice_key: string; sport?: SportState;
  }>("snapshot");
  if (snap.sport) sport = snap.sport;
  dock = snap.dock;
  voiceKey = snap.voice_key;
  micBtn.title = voiceKey ? `Sprachassistent (${voiceKey})` : "Sprachassistent";
  fullscreen = snap.fullscreen;
  peek = !!snap.peek;
  hover = hoverRaw = snap.hover;
  setMedia(snap.media);
  setCover(snap.cover);
  acts = snap.activities;
  watchFolio(acts);
  shelf = snap.shelf;
  clips = snap.clips ?? [];
  clips.forEach((c) => seenClips.add(c.id));
  clipsReadyAt = Date.now() + 1500;
  dragIcon = await invoke<string>("drag_icon").catch(() => "");
  renderPresets(); // seitlich sind alle Bereiche gleichzeitig sichtbar
  render();

  // Tabs
  document.querySelectorAll<HTMLButtonElement>(".tab").forEach((t) =>
    t.addEventListener("click", () => setView(t.dataset.view as View)),
  );
  clearTray.addEventListener("click", () => invoke("shelf_clear"));
  q(".clips-clear").addEventListener("click", () => invoke("clip_clear"));

  // Sprachassistent: Mikrofon-Knopf, Strg+Alt+Leertaste, Beenden, Schluessel eintragen
  micBtn.addEventListener("click", () => void toggleVoice());
  await listen("voice-toggle", () => void toggleVoice());
  // Controller: Steuerkreuz links + R3 — nach 250 ms Ladering, nach 1,5 s zuhoeren, loslassen = antworten
  await listen<number>("voice-arm", (e) => {
    clearTimeout(armTimer);
    armMs = e.payload;
    armUntil = e.payload > 0 ? Date.now() + e.payload : 0;
    // Sicherheitsnetz: kommt das Ende nicht an, Ring trotzdem wegnehmen
    if (armUntil) armTimer = window.setTimeout(() => { armUntil = 0; render(); }, e.payload + 1500);
    render();
  });
  await listen<boolean>("voice-ptt", async (e) => {
    if (e.payload) {
      if (!voice.active && !(await invoke<boolean>("voice_ready"))) { padVoice = true; showVoice("nokey"); padVoice = false; return; }
      padVoice = true;
    } else if (!padVoice) return;
    void voice.hold(e.payload);
    render();
  });
  await listen<"off" | "compact" | "expanded">("pad", (e) => { padMode = e.payload; render(); });

  // Live-Sport: Spielstaende, neue Meldungen, Ballverlauf des angesehenen Spiels
  await listen<SportState>("sport", (e) => { sport = e.payload; render(); });
  await listen<SportNews>("sport-news", (e) => onSportNews(e.payload));
  await listen<{ key: string; reset: boolean; plays: SportPlay[] }>("sport-plays", (e) => {
    const p = e.payload;
    if (p.key !== pitch.matchKey) return;
    if (p.reset) pitch.reset(p.plays); else pitch.add(p.plays);
  });
  q(".v-stop").addEventListener("click", () => { voice.stop(); showVoice("off"); });
  q(".v-setup").addEventListener("click", () => { invoke("voice_setup"); showVoice("off"); });

  // Player
  player.querySelectorAll<HTMLButtonElement>(".controls button").forEach((b) =>
    b.addEventListener("click", () => {
      const action = b.dataset.act!;
      if (action === "toggle" && media) {
        const pos = media.position + (media.playing ? (Date.now() - mediaAt) / 1000 : 0);
        setMedia({ ...media, playing: !media.playing, position: pos });
        render();
      }
      invoke("media_control", { action });
    }),
  );
  q(".cover-btn").addEventListener("click", () => invoke("media_focus"));
  q(".title-btn").addEventListener("click", () => invoke("media_focus"));
  q(".src").addEventListener("click", () => invoke("media_next_source"));

  makeSlider(
    seekEl,
    (f) => { if (media?.duration) q(".t-pos", player).textContent = fmt(f * media.duration); },
    (f) => {
      if (!media?.duration) return;
      const sec = f * media.duration;
      media = { ...media, position: sec };
      mediaAt = Date.now();
      invoke<boolean>("media_seek", { seconds: sec }).catch(() => {});
    },
  );

  let volTimer = 0;
  const sendVol = (f: number, now = false) => {
    // Geraetename behalten — sonst verschwindet die Zeile kurz und der Player springt
    volume = { ...volume, level: f, muted: f < 0.01 };
    renderVolume();
    clearTimeout(volTimer);
    const go = () => invoke<Volume>("volume_set", { level: f }).then((v) => { volume = v; renderVolume(); }).catch(() => {});
    if (now) go(); else volTimer = window.setTimeout(go, 40);
  };
  makeSlider(volSlider, (f) => sendVol(f), (f) => sendVol(f, true));
  q(".vol-btn").addEventListener("click", () =>
    invoke<Volume>("volume_mute").then((v) => { volume = v; renderVolume(); }),
  );
  // Mausrad ueber dem Player = Lautstaerke
  player.addEventListener("wheel", (e) => {
    e.preventDefault();
    const base = volume.muted ? 0 : volume.level;
    sendVol(Math.min(1, Math.max(0, base + (e.deltaY < 0 ? 0.04 : -0.04))), true);
  }, { passive: false });

  // Ablage-Leiste
  // Rechtsklick-Menue: lebt in der Form, schliesst bei jedem Klick daneben
  q(".shape").append(menu);
  document.addEventListener("pointerdown", (e) => { if (!menu.contains(e.target as Node)) closeMenu(); }, true);
  // Klick ins Leere der Ablage hebt die Auswahl auf
  filesEl.addEventListener("click", (e) => { if (e.target === filesEl) { sel.clear(); applySel(); } });

  // Die Form selbst (nicht Knoepfe/Regler/Dateien) mit gedrueckter Maus ziehen -> an eine andere Kante andocken
  const INTERACTIVE = "button, .slider, .file, .files, .file-bar, .ctx, .act, .wheel, .timer-pick, .clips, .drop, .share-qr, .sport, .sp-news";
  q(".shape").addEventListener("pointerdown", (e) => {
    if (e.button !== 0 || (e.target as Element).closest(INTERACTIVE)) return;
    const x0 = e.clientX, y0 = e.clientY;
    const mv = (ev: PointerEvent) => {
      if (Math.hypot(ev.clientX - x0, ev.clientY - y0) < 12) return;
      window.removeEventListener("pointermove", mv);
      invoke("dock_drag_start");
    };
    window.addEventListener("pointermove", mv);
    window.addEventListener("pointerup", () => window.removeEventListener("pointermove", mv), { once: true });
  });

  document.addEventListener("contextmenu", (e) => e.preventDefault());

  let bgTick = 0;
  setInterval(() => {
    tickWatch();
    // Konferenz: mehrere laufende Spiele wechseln sich in der Mitte alle 8 s ab
    if (bgTick % 16 === 0 && centerCandidates().length > 1) { rotate++; render(false); }
    if (state === "expanded") { tickProgress(); tickClock(); tickAges(); }
    // Verlaufs-Eintraege altern auch ohne neue Meldung: "vor X Min", grau ab 12 Min (alle 10 s pruefen)
    if (++bgTick % 20 === 0 && acts.some(isBg)) {
      for (const [, row] of rows) { const a = rowAct.get(row); if (a && isBg(a)) fillRow(row, a); }
      renderCompact();
      render(false);
    }
    // Pause-Nachlauf abgelaufen -> zurueck auf idle
    if (media && !media.playing && pausedAt && Date.now() - pausedAt > pauseLinger() && state === "compact") {
      pausedAt = 0;
      render();
    }
  }, 500);
}

main();
