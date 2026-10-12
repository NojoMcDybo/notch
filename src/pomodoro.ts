/**
 * Pomodoro: eigene Karte fuer den eingebauten Timer (timer.rs, Feld `pomo`) und die Einstellung im Timer-Reiter.
 *
 * Ablauf: Fokus zaehlt runter -> ist er um, haelt nichts an: ein weicher Dreiklang, der Ring ist leer und ein
 * kleiner Punkt in der Farbe der Pause kreist um ihn, die Zeit zaehlt als Extrazeit hoch („+1:24“), der Knopf
 * „Pause starten“ atmet in Mint. Klick darauf: Farbe gleitet von Koralle zu Mint, der Ring fuellt sich in einem
 * Zug und laeuft wieder ab. Nach der Pause genauso zurueck in den Fokus.
 */
import { invoke } from "@tauri-apps/api/core";

export type PomoInfo = {
  phase: "work" | "break";
  /** ms seit 1970, seit die Phase um ist (Extrazeit) */
  over_since: number | null;
  total: number;
  left: number;
  running: boolean;
  round: number;
  work: number;
  brk: number;
  ends_at: number | null;
};
type PAct = {
  id: string; title: string; subtitle?: string; icon?: string;
  actions: { id: string; label: string; icon?: string }[];
  pomo?: PomoInfo;
};

const R = 26;
const C = 2 * Math.PI * R;
const SVG = "http://www.w3.org/2000/svg";

type Ui = {
  arc: SVGCircleElement; time: HTMLElement; title: HTMLElement; sub: HTMLElement; btns: HTMLElement;
  p: PomoInfo; key: string; fillAt: number; actSig: string;
};
const uis = new Map<HTMLElement, Ui>();

const two = (n: number) => String(n).padStart(2, "0");
function clock(s: number) {
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60);
  return h ? `${h}:${two(m)}:${two(s % 60)}` : `${m}:${two(s % 60)}`;
}

function svg<K extends keyof SVGElementTagNameMap>(tag: K, attrs: Record<string, string | number>) {
  const e = document.createElementNS(SVG, tag);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, String(v));
  return e;
}

const div = (cls: string) => { const e = document.createElement("div"); e.className = cls; return e; };

/** Ring links; daneben eine leise Zeile (Phase · Runde) und die Zeit gross; rechts runde Knoepfe */
function build(row: HTMLElement): Ui {
  row.replaceChildren();
  const ring = div("pm-ring");
  const s = svg("svg", { viewBox: "0 0 60 60" });
  const arc = svg("circle", { class: "pm-arc", cx: 30, cy: 30, r: R, transform: "rotate(-90 30 30)" });
  arc.style.strokeDasharray = `${C}`;
  arc.style.strokeDashoffset = `${C}`;
  const orbit = svg("g", { class: "pm-orbit" });
  orbit.append(svg("circle", { class: "pm-dot", cx: 30, cy: 30 - R, r: 3.4 }));
  s.append(svg("circle", { class: "pm-track", cx: 30, cy: 30, r: R }), arc, orbit);
  ring.append(s, div("pm-core"));
  const text = div("pm-text");
  const line = div("pm-line");
  const title = document.createElement("span");
  title.className = "pm-title";
  const sub = document.createElement("span");
  sub.className = "pm-sub";
  line.append(title, sub);
  const time = div("pm-time");
  text.append(line, time);
  const btns = div("pm-btns");
  row.append(ring, text, btns);
  return { arc, time, title, sub, btns, p: null as unknown as PomoInfo, key: "", fillAt: 0, actSig: "" };
}

/** Zeile des Timers als Pomodoro-Karte fuellen; baut nur beim ersten Mal, danach nur aktualisieren */
export function fillPomo(row: HTMLElement, a: PAct, icon: (name: string) => Element | null) {
  const p = a.pomo!;
  let ui = uis.get(row);
  if (!ui || !row.contains(ui.arc)) { ui = build(row); uis.set(row, ui); }
  const over = p.over_since != null;
  // neue Phase (Start, „Pause starten“, Ueberspringen): Ring fuellt sich in einem Zug
  const key = `${p.phase}:${p.round}:${over}`;
  if (!ui.key || (key !== ui.key && !over)) ui.fillAt = performance.now();
  ui.key = key;
  ui.p = p;
  row.dataset.phase = p.phase;
  row.toggleAttribute("data-over", over);
  row.toggleAttribute("data-paused", !over && !p.running);
  ui.title.textContent = a.title;
  ui.sub.textContent = !over && a.subtitle ? a.subtitle : "";

  // in der Extrazeit wird „weiter“ zum einzigen hellen Knopf mit Text; sonst nur leise Symbole
  const sig = over + a.actions.map((x) => x.id + x.label).join("|");
  if (sig !== ui.actSig) {
    ui.actSig = sig;
    ui.btns.replaceChildren(...a.actions.map((ac) => {
      const main = over && ac.id === "next";
      const b = document.createElement("button");
      b.className = main ? "pm-go-next" : "pm-btn";
      b.title = ac.label;
      if (main) b.textContent = ac.label;
      else { const i = icon(ac.icon ?? "") ?? icon(ac.id); if (i) b.append(i); else b.textContent = ac.label; }
      b.addEventListener("click", (e) => { e.stopPropagation(); invoke("activity_action", { id: a.id, action: ac.id }).catch(() => {}); });
      return b;
    }));
  }
  draw(ui);
  loop();
}

function draw(ui: Ui) {
  const p = ui.p;
  const now = Date.now();
  let frac: number, text: string;
  if (p.over_since != null) {
    frac = 0;
    text = "+" + clock(Math.max(0, Math.floor((now - p.over_since) / 1000)));
  } else {
    const left = p.running && p.ends_at ? Math.max(0, (p.ends_at - now) / 1000) : p.left;
    frac = p.total > 0 ? Math.min(1, left / p.total) : 0;
    text = clock(Math.ceil(left));
  }
  if (ui.fillAt) {
    const k = Math.min(1, (performance.now() - ui.fillAt) / 900);
    frac *= 1 - Math.pow(1 - k, 3);
    if (k >= 1) ui.fillAt = 0;
  }
  ui.arc.style.strokeDashoffset = (C * (1 - frac)).toFixed(2);
  if (ui.time.textContent !== text) ui.time.textContent = text;
}

let raf = 0;
let visible: () => boolean = () => true;
/** nur zeichnen, solange man die Karte sieht */
export function pomoWhen(fn: () => boolean) { visible = fn; }

function loop() {
  if (raf) return;
  const step = () => {
    raf = 0;
    for (const [row, ui] of uis) {
      if (!row.isConnected || !row.contains(ui.arc)) { uis.delete(row); continue; }
      if (visible()) draw(ui);
    }
    if (uis.size) raf = requestAnimationFrame(step);
  };
  raf = requestAnimationFrame(step);
}

// ---------- Klang: weicher Dreiklang statt Systemton ----------

let ctx: AudioContext | null = null;
/** `done`: welche Phase gerade fertig ist — Fokus geschafft klingt aufwaerts, Pause vorbei sanft zurueck */
export function pomoChime(done: "work" | "break") {
  try {
    ctx ??= new AudioContext();
    const t0 = ctx.currentTime + 0.02;
    const notes = done === "work" ? [523.25, 659.25, 783.99] : [783.99, 659.25, 587.33];
    notes.forEach((f, i) => {
      const o = ctx!.createOscillator();
      const g = ctx!.createGain();
      o.type = "sine";
      o.frequency.value = f;
      const t = t0 + i * 0.16;
      g.gain.setValueAtTime(0, t);
      g.gain.linearRampToValueAtTime(0.16, t + 0.025);
      g.gain.exponentialRampToValueAtTime(0.0008, t + 1.1);
      o.connect(g).connect(ctx!.destination);
      o.start(t);
      o.stop(t + 1.2);
    });
  } catch { /* ohne Ton, egal */ }
}

// ---------- Einstellen im Timer-Reiter: Fokus und Pause ----------

const KEY = "pomo-pick";
const LIMIT = { work: [1, 180], brk: [1, 60] } as const;
/** Fokus unter 10 Minuten in Einerschritten, darueber in Fuenfern; Pause immer in Einern */
const step = (kind: "work" | "brk", v: number, up: boolean) => (kind === "work" && (v > 10 || (v === 10 && up)) ? 5 : 1);

/** Zwei grosse, leichte Zahlen wie die Drehraeder des Timers: Mausrad, Pfeile oder Ziehen nach oben/unten */
export function pomoPicker(onStart: (work: number, brk: number) => void) {
  const val = { work: 25, brk: 5 };
  try {
    const s = JSON.parse(localStorage.getItem(KEY) ?? "null");
    if (s && typeof s === "object") { val.work = Number(s.work) || 25; val.brk = Number(s.brk) || 5; }
  } catch { /* kein Speicher */ }

  const box = div("pm-pick");
  const field = (kind: "work" | "brk", label: string) => {
    const w = div(`pm-set k-${kind}`);
    const l = div("pm-set-l");
    l.textContent = label;
    const up = document.createElement("button");
    up.className = "pm-step up";
    up.title = "Mehr";
    const down = document.createElement("button");
    down.className = "pm-step down";
    down.title = "Weniger";
    const num = document.createElement("b");
    const v = div("pm-set-v");
    const unit = document.createElement("small");
    unit.textContent = "min";
    v.append(num, unit);
    const show = () => { num.textContent = String(val[kind]); };
    const set = (more: boolean) => {
      const [lo, hi] = LIMIT[kind];
      const cur = val[kind];
      const st = step(kind, cur, more);
      const n = more ? Math.floor(cur / st) * st + st : Math.ceil(cur / st) * st - st;
      const next = Math.min(hi, Math.max(lo, n));
      if (next === cur) return;
      val[kind] = next;
      show();
      num.classList.remove("tick-up", "tick-down"); void num.offsetWidth; num.classList.add(more ? "tick-up" : "tick-down");
      try { localStorage.setItem(KEY, JSON.stringify(val)); } catch { /* egal */ }
    };
    up.addEventListener("click", () => set(true));
    down.addEventListener("click", () => set(false));
    w.addEventListener("wheel", (e) => { e.preventDefault(); set(e.deltaY < 0); }, { passive: false });
    w.append(l, up, v, down);
    show();
    return w;
  };
  const go = document.createElement("button");
  go.className = "pick-start pm-start";
  go.textContent = "Start";
  go.addEventListener("click", () => onStart(val.work, val.brk));
  box.append(field("work", "Fokus"), field("brk", "Pause"), go);
  return box;
}
