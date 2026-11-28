/**
 * iPhone-Kamera in der aufgeklappten Notch (Zeile `notch:iphone:cam`, cam.rs):
 *
 * - Vorschau je Geraet, das gerade sendet (Safari-Seite, Notch-App, Bildschirm). Mehrere Geraete duerfen gleichzeitig
 *   senden; die virtuelle Kamera „Notch iPhone“ zeigt nur das Live-Geraet (gruener Rand). Klick auf eine Vorschau
 *   schaltet sie live.
 * - Darunter die Einstellungen des Live-Geraets wie am iPhone (Frontkamera, Licht, Spiegeln, Objektiv, Zoom, Qualitaet,
 *   Bildrate). Beidseitig: was hier geaendert wird, holt sich das Geraet mit seiner naechsten Antwort ab; was am
 *   Geraet geaendert wird, erscheint hier.
 * - Bilder und Liste werden nur geholt, solange die Notch offen ist.
 */
import { invoke } from "@tauri-apps/api/core";
import "./camcard.css";

type Caps = { lenses?: string[]; zoom?: { min: number; max: number } | null; torch?: boolean };
type Settings = { front?: boolean; lens?: string; quality?: number; rate?: number; mirror?: boolean; zoom?: number; torch?: boolean };
type Source = {
  key: string; name: string; live: boolean; active: boolean; fps: number; seq: number; screen: boolean;
  settings: Settings; caps: Caps;
};
type CAct = { id: string; title: string; subtitle?: string };

const LENS_NAME: Record<string, string> = { ultra: "Ultraweit", wide: "Weit", tele: "Tele" };
/** Liste alle 400 ms, Bilder hoechstens ~8 je Sekunde und Geraet */
const LIST_MS = 400;
const FRAME_MS = 120;
/** nach einer eigenen Aenderung so lange nicht von der Liste ueberschreiben (bis das Geraet sie bestaetigt hat) */
const HOLD_MS = 2500;

type Tile = { el: HTMLElement; img: HTMLImageElement; name: HTMLElement; badge: HTMLElement; seq: number; at: number };
type Ui = {
  row: HTMLElement; title: HTMLElement; sub: HTMLElement; tiles: HTMLElement; ctl: HTMLElement;
  map: Map<string, Tile>; list: Source[]; ctlSig: string;
};

let ui: Ui | null = null;
let when: () => boolean = () => true;
let resized: () => void = () => {};
let timer = 0;

const div = (cls: string, text?: string) => {
  const e = document.createElement("div");
  e.className = cls;
  if (text != null) e.textContent = text;
  return e;
};

/** wird von main.ts gesetzt: Bilder nur holen, solange die Notch aufgeklappt ist */
export function camWhen(fn: () => boolean, onResize: () => void) {
  when = fn;
  resized = onResize;
}

function build(row: HTMLElement): Ui {
  row.replaceChildren();
  const head = div("cc-head");
  const title = div("cc-title");
  const sub = div("cc-sub");
  head.append(title, sub);
  const tiles = div("cc-tiles");
  const ctl = div("cc-ctl");
  row.append(head, tiles, ctl);
  // Klicks in der Karte oeffnen nichts und starten kein Ziehen der Karte (cards.ts)
  row.addEventListener("click", (e) => e.stopPropagation());
  return { row, title, sub, tiles, ctl, map: new Map(), list: [], ctlSig: "" };
}

/** Zeile der Kamera als Karte fuellen; baut einmal, aktualisiert sich danach selbst */
export function fillCam(row: HTMLElement, a: CAct) {
  if (!ui || ui.row !== row || !row.contains(ui.tiles)) ui = build(row);
  ui.title.textContent = a.title;
  ui.sub.textContent = a.subtitle ?? "";
  if (!timer) timer = window.setInterval(poll, LIST_MS);
  void poll();
}

let polling = false;
async function poll() {
  const u = ui;
  if (!u || !u.row.isConnected) {
    window.clearInterval(timer);
    timer = 0;
    ui = null;
    return;
  }
  if (!when() || polling) return;
  polling = true;
  try {
    const list = await invoke<Source[]>("cam_sources");
    overlay(list);
    u.list = list;
    const before = u.row.offsetHeight;
    drawTiles(u);
    drawControls(u);
    if (u.row.offsetHeight !== before) resized();
    await frames(u);
  } catch {
    /* Notch beendet die Kamera gerade: beim naechsten Mal wieder */
  } finally {
    polling = false;
  }
}

// ---------- Vorschauen ----------

function drawTiles(u: Ui) {
  const keys = new Set(u.list.map((s) => s.key));
  for (const [k, t] of u.map) if (!keys.has(k)) { t.el.remove(); u.map.delete(k); }
  u.tiles.dataset.n = String(Math.min(u.list.length, 3));
  for (const s of u.list) {
    let t = u.map.get(s.key);
    if (!t) {
      const el = div("cc-tile");
      const img = document.createElement("img");
      img.alt = "";
      img.draggable = false;
      const name = div("cc-name");
      const badge = div("cc-badge");
      el.append(img, name, badge);
      el.addEventListener("click", () => {
        const cur = u.list.find((x) => x.key === s.key);
        if (!cur || cur.live) return;
        // sofort zeigen, die Notch bestaetigt mit der naechsten Liste
        u.list.forEach((x) => (x.live = x.key === s.key));
        drawTiles(u);
        drawControls(u);
        invoke("cam_live", { key: s.key }).catch(() => {});
      });
      t = { el, img, name, badge, seq: -1, at: 0 };
      u.map.set(s.key, t);
      u.tiles.append(el);
    }
    t.el.classList.toggle("live", s.live);
    t.el.classList.toggle("idle", !s.active);
    t.el.title = s.live ? "Live in der Kamera „Notch iPhone“" : "Klicken: dieses Gerät live schalten";
    t.name.textContent = s.name || (s.screen ? "Bildschirm" : "Gerät");
    t.badge.textContent = s.live ? (s.fps ? `LIVE · ${s.fps}` : "LIVE") : s.active ? "Vorschau" : "Pause";
  }
}

async function frames(u: Ui) {
  const now = performance.now();
  for (const s of u.list) {
    const t = u.map.get(s.key);
    if (!t || t.seq === s.seq || now - t.at < FRAME_MS) continue;
    t.at = now;
    const b64 = await invoke<string | null>("cam_preview", { key: s.key }).catch(() => null);
    if (b64) {
      t.img.src = `data:image/jpeg;base64,${b64}`;
      t.seq = s.seq;
    }
  }
}

// ---------- Einstellungen des Live-Geraets ----------

/** eigene Aenderungen, bis das Geraet sie bestaetigt hat (sonst springt ein Schalter kurz auf den alten Wert) */
const pending = new Map<string, { patch: Settings; at: number }>();

function remember(s: Source, patch: Settings) {
  Object.assign(s.settings, patch);
  const p = pending.get(s.key);
  pending.set(s.key, { patch: { ...(p?.patch ?? {}), ...patch }, at: performance.now() });
}

/** in die frisch geholte Liste die noch nicht bestaetigten eigenen Aenderungen eintragen */
function overlay(list: Source[]) {
  const now = performance.now();
  for (const s of list) {
    if (!s.settings || typeof s.settings !== "object") s.settings = {};
    const p = pending.get(s.key);
    if (!p) continue;
    if (now - p.at > HOLD_MS) pending.delete(s.key);
    else Object.assign(s.settings, p.patch);
  }
}

function set(u: Ui, s: Source, patch: Settings) {
  remember(s, patch);
  invoke("cam_config", { key: s.key, patch }).catch(() => {});
  drawControls(u, true);
}

function seg<T extends string | number>(opts: [T, string][], cur: T | undefined, pick: (v: T) => void) {
  const box = div("cc-seg");
  for (const [v, label] of opts) {
    const b = document.createElement("button");
    b.type = "button";
    b.textContent = label;
    b.setAttribute("aria-pressed", String(v === cur));
    b.addEventListener("click", () => { if (v !== cur) pick(v); });
    box.append(b);
  }
  return box;
}

function chip(label: string, on: boolean, disabled: boolean, toggle: () => void) {
  const b = document.createElement("button");
  b.type = "button";
  b.className = "cc-chip";
  b.textContent = label;
  b.setAttribute("aria-pressed", String(on));
  b.disabled = disabled;
  b.addEventListener("click", toggle);
  return b;
}

const times = (z: number) => `${z.toFixed(1).replace(".", ",")}×`;

function drawControls(u: Ui, force = false) {
  const s = u.list.find((x) => x.live);
  // waehrend man am Zoom zieht, nicht neu aufbauen
  if (!force && u.ctl.contains(document.activeElement) && document.activeElement instanceof HTMLInputElement) return;
  const sig = s ? JSON.stringify([s.key, s.screen, s.settings, s.caps]) : "";
  if (sig === u.ctlSig && !force) return;
  u.ctlSig = sig;
  u.ctl.replaceChildren();
  if (!s) return;
  if (s.screen) {
    u.ctl.append(div("cc-note", "Bildschirmübertragung: keine Kamera-Einstellungen."));
    return;
  }
  const st = s.settings, caps = s.caps ?? {};
  const front = !!st.front;

  const chips = div("cc-row cc-chips");
  chips.append(
    chip("Frontkamera", front, false, () => set(u, s, { front: !front, torch: false, zoom: 1 })),
    chip("Licht", !!st.torch, front || !caps.torch, () => set(u, s, { torch: !st.torch })),
    chip("Spiegeln", !!st.mirror, false, () => set(u, s, { mirror: !st.mirror })),
  );
  u.ctl.append(chips);

  const lenses = (caps.lenses ?? []).filter((l) => l in LENS_NAME);
  if (!front && lenses.length > 1) {
    const r = div("cc-row");
    r.append(seg(lenses.map((l) => [l, LENS_NAME[l]] as [string, string]), st.lens ?? "wide", (v) => set(u, s, { lens: v, zoom: 1, torch: false })));
    u.ctl.append(r);
  }

  if (caps.zoom && caps.zoom.max > caps.zoom.min) {
    const r = div("cc-row cc-zoom");
    const inp = document.createElement("input");
    inp.type = "range";
    inp.min = String(caps.zoom.min);
    inp.max = String(Math.min(caps.zoom.max, 8));
    inp.step = "0.1";
    inp.value = String(st.zoom ?? caps.zoom.min);
    const val = div("cc-val", times(Number(inp.value)));
    let t = 0;
    inp.addEventListener("input", () => {
      const z = Number(inp.value);
      val.textContent = times(z);
      remember(s, { zoom: z });
      // beim Ziehen gebuendelt senden
      window.clearTimeout(t);
      t = window.setTimeout(() => invoke("cam_config", { key: s.key, patch: { zoom: z } }).catch(() => {}), 80);
    });
    inp.addEventListener("change", () => inp.blur());
    r.append(div("cc-lbl", "Zoom"), inp, val);
    u.ctl.append(r);
  }

  const r2 = div("cc-row cc-pair");
  const q = div("cc-field");
  q.append(div("cc-lbl", "Qualität"), seg<number>([[0, "Niedrig"], [1, "Normal"], [2, "Hoch"]], st.quality ?? 1, (v) => set(u, s, { quality: v })));
  const f = div("cc-field");
  f.append(div("cc-lbl", "Bilder/s"), seg<number>([[15, "15"], [30, "30"]], st.rate ?? 30, (v) => set(u, s, { rate: v })));
  r2.append(q, f);
  u.ctl.append(r2);
}
