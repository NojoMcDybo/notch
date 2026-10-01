import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { startDrag } from "@crabnebula/tauri-plugin-drag";
import { Voice, type VoiceState } from "./voice";

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
};
type Item = { path: string; name: string; ext: string; size: number; kind: string; added: number };
type Target = { id: string; label: string };
type Volume = { level: number; muted: boolean; device?: string; follows_player?: boolean };

type State = "idle" | "compact" | "expanded";
type View = "home" | "tray" | "timer";

type Dock = "top" | "left" | "right";

const EAR = 10;
const EAR_X = 14;
const MAX_H = 380;
/** oben: waagerecht wie die Mac-Notch */
const SIZE = {
  idle: { w: 190, h: 30, r: 10 },
  compact: { w: 320, h: 34, r: 13 },
  expanded: { w: 520, h: 0, r: 30 },
};
/** seitlich: senkrechte Pille, aufgeklappt ein hohes Panel mit allem auf einmal */
const SIDE = {
  idle: { w: 30, h: 180, r: 10 },
  compact: { w: 44, h: 280, r: 15 },
  expanded: { w: 400, h: 0, r: 28 },
};
/** so lange bleibt die Notch nach Pause noch kompakt */
const PAUSE_LINGER = 30_000;
const PRESETS = [1, 3, 5, 10, 15, 25, 45, 60];

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

// ---------- Zustand ----------

let media: Media = null;
let mediaAt = 0;
let pausedAt = 0;
let coverSrc: string | null = null;
let acts: Activity[] = [];
let shelf: Item[] = [];
let sel = new Set<string>();
let anchor: string | null = null;
let convertStatus = "";
let volume: Volume = { level: 0.5, muted: false };
let hover = false;
let hoverRaw = false;
let peekUntil = 0;
let fullscreen = false;
let dragOver = false;
let holding = 0; // laufende Zieh-Gesten (Spulen/Lautstaerke) halten die Notch offen
let state: State = "idle";
let view: View = "home";
let hoverTimer = 0;
let dragIcon = "";
let dock: Dock = "top";
let voiceShown = false;
let voiceKey = "";

// ---------- Sprachassistent ----------

const voiceEl = q(".voice");
const orb = q(".orb");
const micBtn = q(".mic-btn");
const V_LABEL: Record<string, string> = {
  connecting: "Verbinde …",
  listening: "Hört zu",
  thinking: "Denkt nach …",
  speaking: "Spricht",
  error: "Fehler",
  nokey: "OpenAI-Schlüssel fehlt",
};

function showVoice(s: VoiceState | "nokey", detail = "") {
  voiceShown = s !== "off";
  voiceEl.hidden = !voiceShown;
  voiceEl.dataset.s = s;
  q(".v-state", voiceEl).textContent = V_LABEL[s] ?? "";
  if (detail || s === "error" || s === "nokey") {
    q(".v-text", voiceEl).textContent =
      s === "nokey" ? `Einmal eintragen, danach mit ${voiceKey || "dem Mikrofon"} starten.` : detail;
  }
  q(".v-setup", voiceEl).hidden = s !== "nokey";
  micBtn.classList.toggle("on", voice.active);
  if (s === "listening" || s === "connecting") { view = "home"; notch.dataset.view = "home"; }
  render(false);
}

const findShelf = (name: string) => {
  const n = name.toLowerCase().trim();
  return shelf.find((i) => i.name.toLowerCase() === n) ?? shelf.find((i) => i.name.toLowerCase().includes(n));
};

const voice = new Voice({
  onState: (s, detail) => showVoice(s, detail),
  onTranscript: (t) => { q(".v-text", voiceEl).textContent = t; render(false); },
  onLevel: (l) => orb.style.setProperty("--lvl", l.toFixed(3)),
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
  if (voice.active) { voice.stop(); return; }
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
    const img = new Image();
    img.src = i;
    box.append(img);
  } else {
    box.textContent = i || (a.app || a.title).slice(0, 1).toUpperCase();
  }
  return box;
}

function eq(playing: boolean) {
  const e = el("div", "eq" + (playing ? "" : " paused"));
  for (let i = 0; i < 4; i++) e.append(el("i"));
  return e;
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
  return media.playing || Date.now() - pausedAt < PAUSE_LINGER;
}

/** Timer abgelaufen und noch nicht bestaetigt? Dann bleibt die Notch offen und pulsiert. */
const timerAlarm = () => acts.some((a) => a.id === "notch:timer" && a.title.includes("abgelaufen"));

function wanted(): State {
  if (hover || dragOver || holding > 0 || voiceShown || timerAlarm() || Date.now() < peekUntil) return "expanded";
  if (musicVisible() || acts.length) return "compact";
  return "idle";
}

let compactSig = "";
let actsSig = "";
let shelfSig = "";

function renderCompact() {
  const top = acts[0];
  // nur neu bauen, wenn sich etwas Sichtbares aendert (sonst startet der Pegel jede Sekunde neu)
  const sig = [musicVisible(), media?.playing, coverSrc?.length, top?.id, top?.value, top?.unit, top?.color, top?.icon].join("|");
  if (sig === compactSig) return;
  compactSig = sig;
  lead.replaceChildren();
  trail.replaceChildren();

  if (musicVisible()) {
    if (coverSrc) {
      const img = new Image();
      img.src = coverSrc;
      lead.append(img);
    } else lead.append(el("div", "icon", "♪"));
  } else if (top) lead.append(iconEl(top));

  if (top?.value) {
    trail.style.setProperty("--accent", top.color ?? "");
    trail.append(el("span", "", top.value));
    if (top.unit) trail.append(el("small", "", top.unit));
  } else if (musicVisible()) {
    trail.style.removeProperty("--accent");
    trail.append(eq(!!media?.playing));
  }
}

function renderPlayer() {
  const show = !!media;
  player.hidden = !show;
  idleInfo.hidden = show || acts.length > 0;
  if (!media) return;
  q(".title", player).textContent = media.title;
  const app = appName(media.source);
  q(".artist", player).textContent = media.artist || (media.title === app ? "bereit" : app);
  const src = q<HTMLButtonElement>(".src", player);
  src.textContent = app;
  src.classList.toggle("multi", media.sessions > 1);
  src.title = media.sessions > 1 ? "Andere Quelle zeigen" : "";
  player.classList.toggle("playing", media.playing);
  if (coverSrc) { if (cover.getAttribute("src") !== coverSrc) cover.src = coverSrc; }
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
  // Welcher Ausgang geregelt wird (z. B. "SteelSeries Sonar - Media"); Zusatz in Klammern weglassen
  const dev = (volume.device ?? "").replace(/\s*\(.*\)\s*$/, "");
  const devEl = q(".vol-dev");
  devEl.textContent = dev;
  devEl.title = volume.follows_player
    ? `Lautstärke des Ausgangs, auf dem der Player spielt: ${volume.device}`
    : `Windows-Standardausgang: ${volume.device ?? ""}`;
}

function renderActs() {
  const sig = JSON.stringify(acts.slice(0, 4));
  if (sig === actsSig) return;
  actsSig = sig;
  actsEl.replaceChildren(
    ...acts.slice(0, 4).map((a) => {
      const row = el("div", "act" + (a.open ? " link" : ""));
      if (a.color) row.style.setProperty("--accent", a.color);
      if (a.open) {
        row.title = "Öffnen";
        row.addEventListener("click", () => invoke("activity_open", { id: a.id }));
      }
      const text = el("div", "act-text");
      text.append(el("div", "act-title", a.title));
      const sub = a.subtitle || (a.app !== a.title ? a.app : "");
      if (sub) text.append(el("div", "act-sub", sub));
      const val = el("div", "act-val", a.value ?? "");
      if (a.unit && a.value) val.append(el("small", "", a.unit));
      row.append(iconEl(a), text, val);

      const x = el("button", "act-x");
      x.title = "Entfernen";
      x.append(svgIcon("close")!);
      x.addEventListener("click", (e) => { e.stopPropagation(); invoke("dismiss_activity", { id: a.id }); });
      row.append(x);

      if (a.progress != null) {
        const p = el("div", "act-prog" + (a.progress < 0 ? " busy" : ""));
        const s = el("span");
        s.style.width = `${Math.round(Math.min(1, Math.max(0, a.progress)) * 100)}%`;
        p.append(s);
        row.append(p);
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
        row.append(bar);
      }
      row.addEventListener("contextmenu", (e) => { e.preventDefault(); invoke("dismiss_activity", { id: a.id }); });
      return row;
    }),
  );
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
  convertStatus = "";
  applySel();
}

/** Auswahl nur per Klasse umschalten — kein Neuaufbau, sonst geht der Doppelklick verloren */
function applySel() {
  filesEl.querySelectorAll<HTMLElement>(".file").forEach((f) => f.classList.toggle("sel", sel.has(f.dataset.path!)));
  void renderFileBar();
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
      f.title = it.path;
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
  void renderFileBar();
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
  for (const it of items) invoke("convert", { path: it.path, target: target.id });
  convertStatus = `${items.length > 1 ? items.length + " Dateien" : items[0]?.name} → ${target.label} …`;
  closeMenu();
  void renderFileBar(); // in der Ablage bleiben; Ergebnis taucht hier auf
}

const ACTIONS = {
  open: () => selectedItems().forEach((i) => invoke("open", { target: i.path })),
  reveal: () => { const i = selectedItems()[0]; if (i) invoke("reveal", { path: i.path }); },
  remove: () => { selectedItems().forEach((i) => invoke("shelf_remove", { path: i.path })); sel.clear(); convertStatus = ""; },
};

let fbToken = 0;
async function renderFileBar() {
  const items = selectedItems();
  fileBar.hidden = !items.length && !convertStatus;
  const token = ++fbToken;
  q(".fb-name", fileBar).textContent =
    convertStatus || (items.length === 1 ? items[0].name : `${items.length} Dateien ausgewählt`);
  q(".fb-row", fileBar).hidden = !items.length;
  const box = q(".fb-formats", fileBar);
  if (!items.length) { box.replaceChildren(); render(false); return; }
  const targets = await commonTargets(items);
  if (token !== fbToken) return; // inzwischen anders ausgewaehlt
  box.replaceChildren();
  if (!targets.length) {
    const av = items.some((i) => i.kind === "audio" || i.kind === "video");
    const ff = av ? await invoke<boolean>("ffmpeg_available") : true;
    box.append(el("span", "none", !ff ? "Für Audio/Video fehlt ffmpeg" : items.length > 1 ? "kein gemeinsames Format" : ""));
  }
  for (const t of targets) {
    const b = el("button", "pill", t.label);
    b.title = `In ${t.label} umwandeln — landet neben dem Original`;
    b.addEventListener("click", () => convertSelected(t));
    box.append(b);
  }
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

function renderPresets() {
  const box = q(".presets");
  if (box.childElementCount) return;
  for (const m of PRESETS) {
    const b = el("button", "preset");
    b.append(el("b", "", String(m)), el("small", "", "min"));
    b.addEventListener("click", () => { invoke("timer_start", { seconds: m * 60 }); setView("home"); });
    box.append(b);
  }
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
  if (full) {
    renderCompact();
    renderPlayer();
    renderActs();
    renderShelf();
    renderVolume();
    tickClock();
  }

  state = wanted();
  notch.dataset.state = state;
  notch.classList.toggle("gone", fullscreen);
  notch.classList.toggle("dragging", dragOver);
  notch.classList.toggle("alarm", timerAlarm());
  if (timerAlarm() && view !== "home") { view = "home"; notch.dataset.view = "home"; }

  notch.dataset.dock = dock;
  const side = dock !== "top";
  const s = { ...(side ? SIDE : SIZE)[state] };
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
  invoke("set_hit_rect", { rect: fullscreen ? { x: 0, y: 0, w: 0, h: 0 } : rect });
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
  // Erst zuhoeren, dann den Stand holen — sonst gehen Ereignisse beim Start verloren.
  await listen<Media>("media", (e) => { setMedia(e.payload); render(); });
  await listen<{ key: string; src: string }>("media-cover", (e) => { setCover(e.payload.src); render(); });
  await listen<Activity[]>("activities", (e) => {
    acts = e.payload;
    // Stand der Umwandlung in der Ablage-Leiste zeigen (man bleibt in der Ablage)
    if (convertStatus) {
      const conv = acts.filter((a) => a.id.startsWith("notch:convert:")).sort((a, b) => b.updated - a.updated)[0];
      if (conv?.subtitle === "fertig") convertStatus = `Fertig: ${conv.title}`;
      else if (conv && conv.color === "#ff453a") convertStatus = `Fehler: ${conv.subtitle ?? ""}`;
      void renderFileBar();
    }
    render();
  });
  await listen<Item[]>("shelf", (e) => { shelf = e.payload; render(); });
  await listen<string>("activity-alert", () => {
    peekUntil = Date.now() + 3500;
    render();
    setTimeout(render, 3600);
  });
  await listen<boolean>("fullscreen", (e) => { fullscreen = e.payload; render(); });
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

  const snap = await invoke<{
    media: Media; cover: string | null; activities: Activity[]; shelf: Item[]; fullscreen: boolean; hover: boolean;
    dock: Dock; voice_key: string;
  }>("snapshot");
  dock = snap.dock;
  voiceKey = snap.voice_key;
  micBtn.title = voiceKey ? `Sprachassistent (${voiceKey})` : "Sprachassistent";
  fullscreen = snap.fullscreen;
  hover = hoverRaw = snap.hover;
  setMedia(snap.media);
  setCover(snap.cover);
  acts = snap.activities;
  shelf = snap.shelf;
  dragIcon = await invoke<string>("drag_icon").catch(() => "");
  renderPresets(); // seitlich sind alle Bereiche gleichzeitig sichtbar
  render();

  // Tabs
  document.querySelectorAll<HTMLButtonElement>(".tab").forEach((t) =>
    t.addEventListener("click", () => setView(t.dataset.view as View)),
  );
  clearTray.addEventListener("click", () => invoke("shelf_clear"));

  // Sprachassistent: Mikrofon-Knopf, Strg+Alt+Leertaste, Beenden, Schluessel eintragen
  micBtn.addEventListener("click", () => void toggleVoice());
  await listen("voice-toggle", () => void toggleVoice());
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
    volume = { level: f, muted: f < 0.01 };
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
  q(".fb-open").addEventListener("click", ACTIONS.open);
  q(".fb-reveal").addEventListener("click", ACTIONS.reveal);
  q(".fb-remove").addEventListener("click", ACTIONS.remove);
  // Rechtsklick-Menue: lebt in der Form, schliesst bei jedem Klick daneben
  q(".shape").append(menu);
  document.addEventListener("pointerdown", (e) => { if (!menu.contains(e.target as Node)) closeMenu(); }, true);
  // Klick ins Leere der Ablage hebt die Auswahl auf
  filesEl.addEventListener("click", (e) => { if (e.target === filesEl) { sel.clear(); convertStatus = ""; applySel(); } });

  // Die Form selbst (nicht Knoepfe/Regler/Dateien) mit gedrueckter Maus ziehen -> an eine andere Kante andocken
  const INTERACTIVE = "button, .slider, .file, .files, .file-bar, .ctx, .act, .preset, .drop";
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

  setInterval(() => {
    if (state === "expanded") { tickProgress(); tickClock(); }
    // Pause-Nachlauf abgelaufen -> zurueck auf idle
    if (media && !media.playing && pausedAt && Date.now() - pausedAt > PAUSE_LINGER && state === "compact") {
      pausedAt = 0;
      render();
    }
  }, 500);
}

main();
