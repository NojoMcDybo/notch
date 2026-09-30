import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { startDrag } from "@crabnebula/tauri-plugin-drag";

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
type Volume = { level: number; muted: boolean };

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
let selected: string | null = null;
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

function wanted(): State {
  if (hover || dragOver || holding > 0 || Date.now() < peekUntil) return "expanded";
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

function renderShelf() {
  badge.hidden = shelf.length === 0;
  badge.textContent = String(shelf.length);
  trayView.classList.toggle("has-files", shelf.length > 0);
  if (selected && !shelf.some((i) => i.path === selected)) selected = null;

  const sig = JSON.stringify(shelf.map((i) => i.path)) + "|" + selected;
  if (sig !== shelfSig) {
    shelfSig = sig;
    filesEl.replaceChildren(
      ...shelf.map((it) => {
        const f = el("div", "file" + (it.path === selected ? " sel" : ""));
        f.title = it.path;
        f.append(fileThumb(it), el("div", "fname", it.name));
        f.addEventListener("click", () => { selected = selected === it.path ? null : it.path; render(); });
        f.addEventListener("dblclick", () => invoke("open", { target: it.path }));
        f.addEventListener("contextmenu", (e) => { e.preventDefault(); invoke("shelf_remove", { path: it.path }); });
        // Rausziehen: ab 6 px Bewegung uebernimmt Windows das Ziehen
        f.addEventListener("pointerdown", (e) => {
          if (e.button !== 0) return;
          const x0 = e.clientX, y0 = e.clientY;
          const mv = (ev: PointerEvent) => {
            if (Math.hypot(ev.clientX - x0, ev.clientY - y0) < 6) return;
            window.removeEventListener("pointermove", mv);
            if (dragIcon) startDrag({ item: [it.path], icon: dragIcon }).catch(() => {});
          };
          window.addEventListener("pointermove", mv);
          window.addEventListener("pointerup", () => window.removeEventListener("pointermove", mv), { once: true });
        });
        return f;
      }),
    );
    renderFileBar();
  }
}

async function renderFileBar() {
  const it = shelf.find((i) => i.path === selected);
  fileBar.hidden = !it;
  if (!it) { render(false); return; }
  q(".fb-name", fileBar).textContent = it.name;
  const box = q(".fb-formats", fileBar);
  box.replaceChildren();
  const targets = await invoke<Target[]>("convert_targets", { path: it.path });
  if (!targets.length) {
    const ff = await invoke<boolean>("ffmpeg_available");
    box.append(el("span", "none", (it.kind === "audio" || it.kind === "video") && !ff ? "Für Audio/Video fehlt ffmpeg" : ""));
  }
  for (const t of targets) {
    const b = el("button", "pill", t.label);
    b.title = `In ${t.label} umwandeln — landet neben dem Original`;
    b.addEventListener("click", () => {
      invoke("convert", { path: it.path, target: t.id });
      setView("home");
    });
    box.append(b);
  }
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

  notch.dataset.dock = dock;
  const side = dock !== "top";
  const s = { ...(side ? SIDE : SIZE)[state] };
  if (state === "expanded") {
    expanded.style.setProperty("--xw", `${s.w}px`);
    const max = side ? Math.floor(innerHeight * 0.92) : MAX_H;
    s.h = Math.min(max, Math.ceil(expanded.scrollHeight));
  }
  const e = state === "expanded" ? EAR_X : EAR;
  notch.style.setProperty("--w", `${s.w}px`);
  notch.style.setProperty("--h", `${s.h}px`);
  notch.style.setProperty("--r", `${s.r}px`);
  notch.style.setProperty("--e", `${e}px`);
  clearTray.hidden = !((side || view === "tray") && shelf.length > 0);

  // Wo die Form im Fenster liegt -> nur dort faengt das Fenster die Maus
  const rect =
    dock === "top"
      ? { x: (innerWidth - s.w) / 2 - e, y: 0, w: s.w + 2 * e, h: s.h }
      : { x: dock === "left" ? 0 : innerWidth - s.w, y: (innerHeight - s.h) / 2 - e, w: s.w, h: s.h + 2 * e };
  invoke("set_hit_rect", { rect: fullscreen ? { x: 0, y: 0, w: 0, h: 0 } : rect });
}

function setHover(v: boolean) {
  const was = state === "expanded";
  hover = v;
  if (v && !was) {
    // Aufklappen: Lautstaerke frisch holen; Timer-Auswahl nicht wieder anzeigen
    if (view === "timer") { view = "home"; notch.dataset.view = "home"; }
    invoke<Volume>("volume_get").then((v) => { volume = v; renderVolume(); }).catch(() => {});
  }
  if (!v) selected = null;
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
  await listen<Activity[]>("activities", (e) => { acts = e.payload; render(); });
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
    dock: Dock;
  }>("snapshot");
  dock = snap.dock;
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
  q(".fb-open").addEventListener("click", () => selected && invoke("open", { target: selected }));
  q(".fb-reveal").addEventListener("click", () => selected && invoke("reveal", { path: selected }));
  q(".fb-remove").addEventListener("click", () => selected && invoke("shelf_remove", { path: selected }));

  // Die Form selbst (nicht Knoepfe/Regler/Dateien) mit gedrueckter Maus ziehen -> an eine andere Kante andocken
  const INTERACTIVE = "button, .slider, .file, .act, .preset, .drop";
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
