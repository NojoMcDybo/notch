import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type Media = {
  title: string; artist: string; album: string; source: string;
  playing: boolean; position: number; duration: number; key: string;
} | null;

type Activity = {
  id: string; app: string; title: string; subtitle?: string; value?: string; unit?: string;
  icon?: string; color?: string; progress?: number; priority: number; alert: boolean; updated: number;
};

type State = "idle" | "compact" | "expanded";

/** muss zu WIN_W in lib.rs passen */
const WIN_W = 600;
const EAR = 10;
const SIZE = {
  idle: { w: 190, h: 30, r: 10 },
  compact: { w: 310, h: 34, r: 13 },
  expanded: { w: 480, h: 0, r: 30 },
};
/** so lange bleibt die Notch nach Pause noch kompakt */
const PAUSE_LINGER = 30_000;

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

let media: Media = null;
let mediaAt = 0;
let pausedAt = 0;
let coverSrc: string | null = null;
let acts: Activity[] = [];
let hover = false;
let peekUntil = 0;
let fullscreen = false;
let state: State = "idle";
let hoverTimer = 0;

// ---------- Hilfen ----------

const fmt = (s: number) => {
  s = Math.max(0, Math.floor(s));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
};

function el(tag: string, cls?: string, text?: string) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
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

// ---------- Zustand ----------

function musicVisible() {
  if (!media) return false;
  return media.playing || Date.now() - pausedAt < PAUSE_LINGER;
}

function wanted(): State {
  if (hover || Date.now() < peekUntil) return "expanded";
  if (musicVisible() || acts.length) return "compact";
  return "idle";
}

let compactSig = "";
let actsSig = "";

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
    const v = el("span", "", top.value);
    trail.style.setProperty("--accent", top.color ?? "");
    trail.append(v);
    if (top.unit) trail.append(el("small", "", top.unit));
  } else if (musicVisible()) {
    trail.style.removeProperty("--accent");
    trail.append(eq(!!media?.playing));
  }
}

function renderPlayer() {
  const show = !!media;
  player.hidden = !show;
  idleInfo.hidden = show;
  if (!media) return;
  q(".title", player).textContent = media.title;
  q(".artist", player).textContent = media.artist || media.source.replace(/\.exe$/i, "");
  player.classList.toggle("playing", media.playing);
  if (coverSrc) { if (cover.getAttribute("src") !== coverSrc) cover.src = coverSrc; }
  else cover.removeAttribute("src");
  q(".t-dur", player).textContent = media.duration ? fmt(media.duration) : "";
  tickProgress();
}

function renderActs() {
  const sig = JSON.stringify(acts.slice(0, 3));
  if (sig === actsSig) return;
  actsSig = sig;
  actsEl.replaceChildren(
    ...acts.slice(0, 3).map((a) => {
      const row = el("div", "act");
      if (a.color) row.style.setProperty("--accent", a.color);
      const text = el("div", "act-text");
      text.append(el("div", "act-title", a.title));
      if (a.subtitle || a.app) text.append(el("div", "act-sub", a.subtitle ?? a.app));
      const val = el("div", "act-val", a.value ?? "");
      if (a.unit && a.value) val.append(el("small", "", a.unit));
      row.append(iconEl(a), text, val);
      if (a.progress != null) {
        const p = el("div", "act-prog");
        const s = el("span");
        s.style.width = `${Math.round(Math.min(1, Math.max(0, a.progress)) * 100)}%`;
        p.append(s);
        row.append(p);
      }
      // Rechtsklick = wegwischen
      row.addEventListener("contextmenu", (e) => {
        e.preventDefault();
        invoke("dismiss_activity", { id: a.id });
      });
      return row;
    }),
  );
}

function tickProgress() {
  if (!media) return;
  let pos = media.position;
  if (media.playing) pos += (Date.now() - mediaAt) / 1000;
  if (media.duration) pos = Math.min(pos, media.duration);
  q<HTMLElement>(".bar span", player).style.width = media.duration ? `${(pos / media.duration) * 100}%` : "0";
  q(".t-pos", player).textContent = media.duration ? fmt(pos) : "live";
}

function tickClock() {
  const d = new Date();
  q(".clock").textContent = d.toLocaleTimeString("de-DE", { hour: "2-digit", minute: "2-digit" });
  q(".date").textContent = d.toLocaleDateString("de-DE", { weekday: "long", day: "numeric", month: "long" });
}

function render() {
  renderCompact();
  renderPlayer();
  renderActs();
  tickClock();

  state = wanted();
  notch.dataset.state = state;
  notch.classList.toggle("gone", fullscreen);

  const s = { ...SIZE[state] };
  if (state === "expanded") {
    expanded.style.setProperty("--xw", `${s.w}px`);
    s.h = Math.min(250, Math.ceil(expanded.scrollHeight));
  }
  notch.style.setProperty("--w", `${s.w}px`);
  notch.style.setProperty("--h", `${s.h}px`);
  notch.style.setProperty("--r", `${s.r}px`);
  notch.style.setProperty("--e", `${state === "expanded" ? 14 : EAR}px`);

  const e = state === "expanded" ? 14 : EAR;
  invoke("set_hit_rect", {
    rect: fullscreen ? { x: 0, y: 0, w: 0, h: 0 } : { x: (WIN_W - s.w) / 2 - e, y: 0, w: s.w + 2 * e, h: s.h },
  });
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
  const snap = await invoke<{ media: Media; cover: string | null; activities: Activity[]; fullscreen: boolean; hover: boolean }>("snapshot");
  fullscreen = snap.fullscreen;
  hover = snap.hover;
  setMedia(snap.media);
  setCover(snap.cover);
  acts = snap.activities;
  render();

  await listen<Media>("media", (e) => { setMedia(e.payload); render(); });
  await listen<{ key: string; src: string }>("media-cover", (e) => { setCover(e.payload.src); render(); });
  await listen<Activity[]>("activities", (e) => { acts = e.payload; render(); });
  await listen<string>("activity-alert", () => {
    peekUntil = Date.now() + 3500;
    render();
    setTimeout(render, 3600);
  });
  await listen<boolean>("fullscreen", (e) => { fullscreen = e.payload; render(); });
  await listen<boolean>("hover", (e) => {
    clearTimeout(hoverTimer);
    // kurz verweilen lassen, damit ein Griff nach der Titelleiste die Notch nicht aufreisst
    hoverTimer = window.setTimeout(() => { hover = e.payload; render(); }, e.payload ? 110 : 260);
  });

  player.querySelectorAll<HTMLButtonElement>("button").forEach((b) =>
    b.addEventListener("click", () => {
      const action = b.dataset.act!;
      if (action === "toggle" && media) {
        setMedia({ ...media, playing: !media.playing, position: media.position + (media.playing ? (Date.now() - mediaAt) / 1000 : 0) });
        render();
      }
      invoke("media_control", { action });
    }),
  );

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
