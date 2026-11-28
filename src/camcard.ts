/**
 * iPhone-Kamera in der aufgeklappten Notch (Zeile `notch:iphone:cam`, cam.rs):
 *
 * - Vorschau je Geraet, das gerade sendet (Safari-Seite, Notch-App, Bildschirm). Mehrere Geraete duerfen gleichzeitig
 *   senden; die virtuelle Kamera „Notch iPhone“ zeigt nur das Live-Geraet (gruener Rand). Klick auf eine Vorschau
 *   schaltet sie live.
 * - Die Bilder kommen ohne Umweg: je Kachel holt eine Schleife das neueste JPEG ueber das Protokoll `camprev`
 *   (cam.rs: preview_protocol). Die Anfrage wartet in der Notch, bis ein neues Bild da ist, so laeuft die Vorschau des
 *   Live-Geraets in dessen Takt (bis 60 Bilder/s); die anderen ruhiger (12/s). Entpackt wird ausserhalb des
 *   Haupt-Threads (createImageBitmap), gezeichnet auf ein Canvas.
 * - Darunter die Einstellungen des Live-Geraets wie am iPhone (Frontkamera, Licht, Spiegeln, Objektiv, Zoom, Qualitaet,
 *   Bildrate). Beidseitig: was hier geaendert wird, bekommt das Geraet mit der Antwort auf sein naechstes Bild; was am
 *   Geraet geaendert wird, erscheint hier.
 * - Liste und Bilder werden nur geholt, solange die Notch offen ist.
 */
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import "./camcard.css";

type Caps = { lenses?: string[]; zoom?: { min: number; max: number } | null; torch?: boolean; maxRate?: number };
type Settings = { front?: boolean; lens?: string; quality?: number; rate?: number; mirror?: boolean; zoom?: number; torch?: boolean };
type Source = {
  key: string; id: number; name: string; live: boolean; active: boolean; fps: number; seq: number; screen: boolean;
  settings: Settings; caps: Caps;
};
type CAct = { id: string; title: string; subtitle?: string };

const LENS_NAME: Record<string, string> = { ultra: "Ultraweit", wide: "Weit", tele: "Tele" };
/** Liste (Namen, live, Einstellungen) alle 400 ms; die Bilder laufen getrennt davon */
const LIST_MS = 400;
/** Vorschau der Geraete, die nicht live sind: so viele Bilder je Sekunde reichen (spart Rechenzeit) */
const IDLE_FPS = 12;
/** nach einer eigenen Aenderung so lange nicht von der Liste ueberschreiben (bis das Geraet sie bestaetigt hat) */
const HOLD_MS = 2500;
/** Zoom beim Ziehen hoechstens so oft schicken; das Geraet gleitet dazwischen weich */
const ZOOM_SEND_MS = 50;

type Tile = {
  key: string; el: HTMLElement; cv: HTMLCanvasElement; name: HTMLElement; badge: HTMLElement;
  /** zuletzt gezeigtes Bild; Schleife laeuft, solange `on` */
  seq: number; at: number; on: boolean;
};
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
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

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

function teardown(u: Ui) {
  for (const t of u.map.values()) t.on = false;
}

/** Zeile der Kamera als Karte fuellen; baut einmal, aktualisiert sich danach selbst */
export function fillCam(row: HTMLElement, a: CAct) {
  if (!ui || ui.row !== row || !row.contains(ui.tiles)) {
    if (ui) teardown(ui);
    ui = build(row);
  }
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
    if (u) teardown(u);
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
  } catch {
    /* Notch beendet die Kamera gerade: beim naechsten Mal wieder */
  } finally {
    polling = false;
  }
}

// ---------- Vorschauen ----------

function drawTiles(u: Ui) {
  const keys = new Set(u.list.map((s) => s.key));
  for (const [k, t] of u.map) if (!keys.has(k)) { t.on = false; t.el.remove(); u.map.delete(k); }
  u.tiles.dataset.n = String(Math.min(u.list.length, 3));
  for (const s of u.list) {
    let t = u.map.get(s.key);
    if (!t) {
      const el = div("cc-tile");
      const cv = document.createElement("canvas");
      const name = div("cc-name");
      const badge = div("cc-badge");
      el.append(cv, name, badge);
      el.addEventListener("click", () => {
        const cur = u.list.find((x) => x.key === s.key);
        if (!cur || cur.live) return;
        // sofort zeigen, die Notch bestaetigt mit der naechsten Liste
        u.list.forEach((x) => (x.live = x.key === s.key));
        drawTiles(u);
        drawControls(u);
        invoke("cam_live", { key: s.key }).catch(() => {});
      });
      t = { key: s.key, el, cv, name, badge, seq: 0, at: 0, on: true };
      u.map.set(s.key, t);
      u.tiles.append(el);
      void stream(u, t, s.id);
    }
    t.el.classList.toggle("live", s.live);
    t.el.classList.toggle("idle", !s.active);
    t.el.title = s.live ? "Live in der Kamera „Notch iPhone“" : "Klicken: dieses Gerät live schalten";
    t.name.textContent = s.name || (s.screen ? "Bildschirm" : "Gerät");
    t.badge.textContent = s.live ? (s.fps ? `LIVE · ${s.fps}` : "LIVE") : s.active ? `Vorschau${s.fps ? ` · ${s.fps}` : ""}` : "Pause";
  }
}

/** Bilder einer Kachel: wartet in der Notch auf das jeweils naechste Bild (long poll), solange die Kachel lebt */
async function stream(u: Ui, t: Tile, id: number) {
  const url = convertFileSrc(String(id), "camprev");
  while (t.on && ui === u) {
    if (!when() || document.hidden) { await sleep(250); continue; }
    const s = u.list.find((x) => x.key === t.key);
    // nicht live: ruhiger, das spart Entpacken und Zeichnen
    const gap = s && !s.live ? 1000 / IDLE_FPS - (performance.now() - t.at) : 0;
    if (gap > 0) await sleep(gap);
    try {
      const r = await fetch(`${url}?after=${t.seq}`, { cache: "no-store" });
      if (r.status !== 200) continue; // 204: kein neues Bild in 0,5 s
      t.seq = Number(r.headers.get("X-Seq")) || t.seq + 1;
      const bmp = await createImageBitmap(await r.blob());
      if (t.on) paint(t.cv, bmp, !!s?.screen);
      bmp.close();
      t.at = performance.now();
    } catch {
      await sleep(500);
    }
  }
}

/** Bild in die Kachel: Kamera fuellt sie (wie die virtuelle Kamera), Bildschirm passt hinein */
function paint(cv: HTMLCanvasElement, bmp: ImageBitmap, fit: boolean) {
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  const w = Math.max(1, Math.round(cv.clientWidth * dpr)), h = Math.max(1, Math.round(cv.clientHeight * dpr));
  if (cv.width !== w || cv.height !== h) { cv.width = w; cv.height = h; }
  const ctx = cv.getContext("2d", { alpha: false });
  if (!ctx) return;
  const s = (fit ? Math.min : Math.max)(w / bmp.width, h / bmp.height);
  const dw = bmp.width * s, dh = bmp.height * s;
  if (fit) { ctx.fillStyle = "#000"; ctx.fillRect(0, 0, w, h); }
  ctx.drawImage(bmp, (w - dw) / 2, (h - dh) / 2, dw, dh);
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
    // beim Ziehen laufend schicken, aber hoechstens alle ZOOM_SEND_MS (der letzte Wert kommt immer an); das Geraet
    // gleitet zum jeweils neuen Wert, so wirkt der Zoom fluessig statt in Spruengen
    let sentAt = 0, t = 0;
    const send = () => { sentAt = performance.now(); invoke("cam_config", { key: s.key, patch: { zoom: s.settings.zoom } }).catch(() => {}); };
    inp.addEventListener("input", () => {
      const z = Number(inp.value);
      val.textContent = times(z);
      remember(s, { zoom: z });
      window.clearTimeout(t);
      const wait = ZOOM_SEND_MS - (performance.now() - sentAt);
      if (wait <= 0) send(); else t = window.setTimeout(send, wait);
    });
    inp.addEventListener("change", () => inp.blur());
    r.append(div("cc-lbl", "Zoom"), inp, val);
    u.ctl.append(r);
  }

  const r2 = div("cc-row cc-pair");
  const q = div("cc-field");
  q.append(div("cc-lbl", "Qualität"), seg<number>([[0, "Niedrig"], [1, "Normal"], [2, "Hoch"]], st.quality ?? 1, (v) => set(u, s, { quality: v })));
  const f = div("cc-field");
  // 60 nur, wenn das Geraet es kann (meldet maxRate)
  const rates: [number, string][] = [[15, "15"], [30, "30"]];
  if ((caps.maxRate ?? 30) >= 60) rates.push([60, "60"]);
  f.append(div("cc-lbl", "Bilder/s"), seg<number>(rates, st.rate ?? 30, (v) => set(u, s, { rate: v })));
  r2.append(q, f);
  u.ctl.append(r2);
}
