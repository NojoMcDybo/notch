/**
 * Einstellungsfenster der Notch (eigenes Fenster, nicht in der Notch).
 * Rangliste der kleinen Notch: ziehen am Griff oder Griff + Pfeiltasten, Schalter = ausblenden,
 * Pfeil = Extras der Quelle. Oben eine Vorschau, die dieselbe Logik benutzt wie die Notch
 * (compact.ts) — live mit echten Daten oder zum Ausprobieren mit Beispielwerten.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { build, bpmOf, needed, plan, planKey, PulseGate, sourceOf, TIMER_ID, type CAct, type Plan } from "./compact";
import { clone, DEFAULTS, normalize, PULSE_HYST, SOURCES, type CompactSettings, type SourceId } from "./settings-model";

const q = <T extends Element = HTMLElement>(s: string, root: ParentNode = document) => root.querySelector(s) as T;
function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
}
function svg(d: string) {
  const s = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  s.setAttribute("viewBox", "0 0 24 24");
  s.setAttribute("aria-hidden", "true");
  const p = document.createElementNS("http://www.w3.org/2000/svg", "path");
  p.setAttribute("d", d);
  s.append(p);
  return s;
}

const GLYPH: Record<SourceId, string> = {
  glucose: "M12 2.6c3.7 4.5 6.6 8.3 6.6 11.7a6.6 6.6 0 0 1-13.2 0c0-3.4 2.9-7.2 6.6-11.7z",
  music: "M19 3v12.2a3 3 0 1 1-2-2.83V7.3l-8 1.6v8.3a3 3 0 1 1-2-2.83V5.4z",
  timer: "M10 2h4v2h-4zm2 4a8 8 0 1 1 0 16 8 8 0 0 1 0-16zm0 2a6 6 0 1 0 0 12 6 6 0 0 0 0-12zm-1 2h2v4.6l2.7 1.6-1 1.7L11 15.7z",
  pulse: "M12 20.6S4 15.8 4 10.2A4.4 4.4 0 0 1 12 7.7a4.4 4.4 0 0 1 8 2.5c0 5.6-8 10.4-8 10.4z",
  folio: "M6 2h8.6L20 7.4V20a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2zm8 1.8V8h4.2zM8 12v1.6h8V12zm0 3.4V17h6v-1.6z",
  other: "M4 4h7v7H4zm9 0h7v7h-7zM4 13h7v7H4zm9 0h7v7h-7z",
};
const GRIP = "M9 5.5a1.5 1.5 0 1 1-3 0 1.5 1.5 0 0 1 3 0zm0 6.5a1.5 1.5 0 1 1-3 0 1.5 1.5 0 0 1 3 0zm0 6.5a1.5 1.5 0 1 1-3 0 1.5 1.5 0 0 1 3 0zm9-13a1.5 1.5 0 1 1-3 0 1.5 1.5 0 0 1 3 0zm0 6.5a1.5 1.5 0 1 1-3 0 1.5 1.5 0 0 1 3 0zm0 6.5a1.5 1.5 0 1 1-3 0 1.5 1.5 0 0 1 3 0z";
const CHEVRON = "M12 15.4 5.6 9 7 7.6l5 5 5-5L18.4 9z";

// ---------- Zustand ----------

let s: CompactSettings = normalize(DEFAULTS);
let mode: "live" | "demo" = "live";
const open = new Set<SourceId>();

type Media = { title: string; artist: string; playing: boolean } | null;
let liveActs: CAct[] = [];
let liveMedia: Media = null;
let liveCover: string | null = null;
let pausedAt = 0;
const liveGate = new PulseGate();

// Beispielwerte zum Ausprobieren
const demoOn = new Set<SourceId>(["glucose", "music", "timer", "pulse", "folio"]);
let demoBpm = 96;
let demoBg = 112;
let demoPage = 12;
let demoTimerLeft = 272;
let demoFlashUntil = 0;
const demoGate = new PulseGate();

const dataUrl = (svgText: string) => `data:image/svg+xml;utf8,${encodeURIComponent(svgText)}`;
const DEMO_COVER = dataUrl(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 60 60"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#ff6b6b"/><stop offset="0.55" stop-color="#845ef7"/><stop offset="1" stop-color="#22223b"/></linearGradient></defs><rect width="60" height="60" fill="url(#g)"/><circle cx="40" cy="22" r="9" fill="#ffd166" opacity="0.9"/></svg>`);
const ico = (d: string, c: string) => dataUrl(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="${c}" d="${d}"/></svg>`);

function demoActs(): CAct[] {
  const now = Date.now();
  const list: CAct[] = [];
  if (demoOn.has("timer")) {
    const m = Math.floor(demoTimerLeft / 60), sec = String(demoTimerLeft % 60).padStart(2, "0");
    list.push({ id: TIMER_ID, app: "Timer", title: "Timer", value: `${m}:${sec}`, color: "#ffb340", icon: ico(GLYPH.timer, "#ffb340"), priority: 5, updated: now });
  }
  if (demoOn.has("glucose")) {
    list.push({
      id: "demo:bg", app: "Haze", title: "Blutzucker", value: String(demoBg), unit: "mg/dL", trend: demoBg > 180 ? "up45" : demoBg < 70 ? "down45" : "flat", delta: 2,
      icon: ico(GLYPH.glucose, "#ff7971"), priority: 1, updated: now, chart: { low: 70, high: 180, points: [[now - 300_000, demoBg - 2], [now - 1000, demoBg]] },
    });
  }
  if (demoOn.has("pulse")) list.push({ id: "demo:puls", app: "Helio", title: "Puls", value: String(demoBpm), unit: "bpm", pulse: demoBpm, color: "#ff5a6e", icon: ico(GLYPH.pulse, "#ff5a6e"), priority: 0, updated: now });
  if (demoOn.has("folio")) list.push({ id: "demo:folio", app: "Folio", title: "Skript.pdf", value: String(demoPage), unit: "/ 240", color: "#ffffff", icon: ico(GLYPH.folio, "#f5f5f7"), priority: 0, updated: now });
  if (demoOn.has("other")) list.push({ id: "demo:export", app: "Export", title: "Video umwandeln", value: "64", unit: "%", color: "#64d2ff", icon: ico(GLYPH.other, "#64d2ff"), priority: 0, updated: now });
  return list.sort((a, b) => b.priority - a.priority);
}

const liveMusic = () => !!liveMedia && (liveMedia.playing || Date.now() - pausedAt < s.music.lingerSec * 1000);

/** Was die Vorschau (und die Markierungen "in der Notch") gerade zeigen */
function current(): { acts: CAct[]; music: boolean; cover: string | null; playing: boolean; plan: Plan } {
  if (mode === "demo") {
    const acts = demoActs();
    const music = demoOn.has("music");
    return { acts, music, cover: DEMO_COVER, playing: true, plan: plan({ acts, music, s, gate: demoGate, folioFlash: Date.now() < demoFlashUntil ? "demo:folio" : null }) };
  }
  const music = liveMusic();
  return { acts: liveActs, music, cover: liveCover, playing: !!liveMedia?.playing, plan: plan({ acts: liveActs, music, s, gate: liveGate }) };
}

// ---------- Speichern ----------

let saveTimer = 0;
function commit(now = true) {
  clearTimeout(saveTimer);
  const go = () => invoke("settings_set", { settings: s }).catch(() => {});
  if (now) go(); else saveTimer = window.setTimeout(go, 150);
  refresh();
}

const announce = (t: string) => { q(".sr").textContent = t; };

// ---------- Vorschau ----------

const stage = q(".stage");
const pvNotch = q(".pv-notch");
const pvLead = q(".c-lead", pvNotch);
const pvTrail = q(".c-trail", pvNotch);
let pvSig = "";
let pvKey = "";

function pvIcon(a: CAct) {
  const box = el("div", "icon");
  const i = a.icon ?? "";
  if (i.startsWith("data:") || i.startsWith("http")) {
    if (!i.startsWith("data:image/svg")) box.classList.add("pic");
    const img = new Image();
    img.src = i;
    box.append(img);
  } else box.textContent = i || (a.app || a.title).slice(0, 1).toUpperCase();
  return box;
}
function pvEq(playing: boolean) {
  const e = el("div", "eq" + (playing ? "" : " paused"));
  for (let i = 0; i < 4; i++) e.append(el("i"));
  return e;
}

function renderPreview(c: ReturnType<typeof current>) {
  const p = c.plan;
  stage.classList.toggle("empty", !p.slots.length && !p.timer);
  const sig = JSON.stringify([planKey(p), p.slots.map((x) => [x.flash, x.act?.value, x.act?.unit]), p.timer?.value, c.playing, c.cover?.length, s.glucose.delta, s.music.eqBesideCover]);
  if (sig === pvSig) return;
  pvSig = sig;
  const key = planKey(p);
  build(pvLead, pvTrail, p, s, { coverSrc: c.cover, musicPlaying: c.playing, iconEl: pvIcon, eq: pvEq }, pvKey !== "" && key !== pvKey);
  pvKey = key;
  const fit = () => pvNotch.style.setProperty("--w", `${needed(pvLead, pvTrail, false, 320)}px`);
  fit();
  for (const img of pvNotch.querySelectorAll("img")) if (!img.complete) img.addEventListener("load", fit, { once: true });
}

function renderNote(c: ReturnType<typeof current>) {
  const note = q(".pv-note");
  if (mode === "demo") { note.textContent = "Schalte an, was gerade laufen soll."; return; }
  const names = [...c.plan.active].map((id) => SOURCES[id].name);
  note.textContent = names.length ? `Gerade aktiv: ${names.join(", ")}` : "Gerade läuft nichts – „Ausprobieren“ zeigt, wie es aussähe.";
}

// ---------- Zeilen: Status und Markierungen ----------

function statusText(id: SourceId, c: ReturnType<typeof current>): string {
  const a = c.acts.find((x) => sourceOf(x) === id);
  switch (id) {
    case "glucose": return a ? `${a.value ?? "–"} ${a.unit ?? ""} · ${a.app}`.replace(/\s+·/, " ·") : "Haze schickt gerade nichts";
    case "music":
      if (mode === "demo") return c.music ? "Spielt · Beispiel-Song" : "Gerade keine Musik";
      if (!liveMedia) return "Gerade keine Musik";
      return `${liveMedia.playing ? "Spielt" : "Pausiert"} · ${liveMedia.title || "unbekannter Titel"}`;
    case "timer": return a ? `${a.title.includes("abgelaufen") ? "Abgelaufen" : "Läuft"} · ${a.value ?? ""}` : "Kein Timer";
    case "pulse": return a ? `${bpmOf(a)} bpm · ${a.app}` : "Helio schickt gerade nichts";
    case "folio": return a ? `${a.title} · Seite ${a.value ?? "?"} ${a.unit ?? ""}`.trim() : "Kein Dokument offen";
    case "other": {
      const o = c.acts.filter((x) => sourceOf(x) === "other");
      return o.length ? o.map((x) => x.title).join(", ") : "Gerade nichts";
    }
  }
}

function tag(cls: string, text: string, title = "") {
  const t = el("span", `tag ${cls}`, text);
  if (title) t.title = title;
  return t;
}

function updateRows(c: ReturnType<typeof current>) {
  const p = c.plan;
  for (const row of list.querySelectorAll<HTMLElement>(".row")) {
    const id = row.dataset.id as SourceId;
    q(".st", row).textContent = statusText(id, c);
    const tags = q(".tags", row);
    const parts: HTMLElement[] = [];
    const slot = p.slots.find((x) => x.src === id);
    if (id === "pulse" && p.boosted.pulse) parts.push(tag("up", "↑ hoher Puls", "Rückt wegen hoher Herzfrequenz nach oben"));
    if (id === "glucose" && p.boosted.glucose) parts.push(tag("up", "↑ außerhalb", "Außerhalb des Zielbereichs: steht ganz oben"));
    if (id === "timer" && p.timer) parts.push(tag("ext", "verbreitert", "Der Timer hängt sich an und verdrängt nichts"));
    if (slot || (id === "timer" && p.timer)) parts.push(tag("now", slot?.flash ? "kurz drin" : "in der Notch"));
    const sig = parts.map((t) => t.textContent).join("|");
    if (tags.dataset.sig !== sig) { tags.dataset.sig = sig; tags.replaceChildren(...parts); }
  }
}

function refresh() {
  const c = current();
  renderPreview(c);
  renderNote(c);
  updateRows(c);
}

// ---------- Bausteine fuer die Extras ----------

/** Abhaengige Optionen (z. B. Schwelle nur, wenn der Schalter an ist) und Texte, die mitlaufen */
let deps: (() => void)[] = [];
const syncDeps = () => deps.forEach((f) => f());

function switchEl(checked: boolean, label: string, onChange: (on: boolean) => void) {
  const l = el("label", "switch");
  const inp = el("input");
  inp.type = "checkbox";
  inp.checked = checked;
  inp.setAttribute("role", "switch");
  inp.setAttribute("aria-label", label);
  l.append(inp, el("i"));
  inp.addEventListener("change", () => onChange(inp.checked));
  return l;
}

function optToggle(label: string, sub: string, get: () => boolean, set: (v: boolean) => void) {
  const o = el("div", "opt");
  o.append(el("b", "", label), el("small", "", sub), switchEl(get(), label, (v) => { set(v); syncDeps(); commit(); }));
  return o;
}

function setFill(r: HTMLInputElement) {
  const p = ((Number(r.value) - Number(r.min)) / (Number(r.max) - Number(r.min))) * 100;
  r.style.setProperty("--p", `${p}%`);
}

function optRange(
  label: string, min: number, max: number, step: number, fmt: (v: number) => string,
  get: () => number, set: (v: number) => void, enabled?: () => boolean, sub?: () => string,
) {
  const o = el("div", "opt range");
  const val = el("span", "val", fmt(get()));
  const r = el("input");
  r.type = "range";
  r.min = String(min); r.max = String(max); r.step = String(step); r.value = String(get());
  r.setAttribute("aria-label", label);
  setFill(r);
  o.append(el("b", "", label), val, r);
  let small: HTMLElement | null = null;
  if (sub) { small = el("small", "", sub()); o.append(small); }
  r.addEventListener("input", () => {
    const v = Number(r.value);
    set(v);
    val.textContent = fmt(v);
    setFill(r);
    syncDeps();
    commit(false);
  });
  r.addEventListener("change", () => commit());
  deps.push(() => {
    if (enabled) o.classList.toggle("dim", !enabled());
    if (small && sub) small.textContent = sub();
  });
  return o;
}

const secs = (v: number) => (v === 0 ? "sofort weg" : `${String(v).replace(".", ",")} s`);

function extras(id: SourceId): HTMLElement[] {
  switch (id) {
    case "glucose": return [
      optToggle("Außerhalb des Zielbereichs ganz nach oben", "Zu niedrig oder zu hoch schlägt alles andere – auch hohen Puls.",
        () => s.glucose.outOfRangeTop, (v) => { s.glucose.outOfRangeTop = v; }),
      optToggle("Änderung anzeigen", "Die Zahl neben dem Pfeil, z. B. +3.", () => s.glucose.delta, (v) => { s.glucose.delta = v; }),
    ];
    case "music": return [
      optToggle("Pegel neben das Cover", "Steht rechts etwas anderes, wandern die Balken nach links statt zu verschwinden.",
        () => s.music.eqBesideCover, (v) => { s.music.eqBesideCover = v; }),
      optRange("Nach Pause noch zeigen", 0, 120, 5, secs, () => s.music.lingerSec, (v) => { s.music.lingerSec = v; }),
    ];
    case "timer": return [
      optToggle("Notch verbreitern, solange ein Timer läuft", "Der Timer hängt sich rechts an und verdrängt nichts. Aus: Er reiht sich hier in die Rangliste ein.",
        () => s.timer.expand, (v) => { s.timer.expand = v; }),
    ];
    case "pulse": return [
      optToggle("Bei hohem Puls nach oben rücken", "Ab der Schwelle steht der Puls ganz oben – nur Blutzucker außerhalb des Bereichs bleibt davor.",
        () => s.pulse.boost, (v) => { s.pulse.boost = v; }),
      optRange("Ab", 90, 200, 5, (v) => `${v} bpm`, () => s.pulse.threshold, (v) => { s.pulse.threshold = v; },
        () => s.pulse.boost, () => `Unter ${s.pulse.threshold - PULSE_HYST} bpm geht er zurück auf Platz ${s.order.indexOf("pulse") + 1}.`),
    ];
    case "folio": return [
      optToggle("Seitenzahl beim Blättern einblenden", "Liegt etwas anderes oben, übernimmt die Seitenzahl kurz den letzten Platz.",
        () => s.folio.flash, (v) => { s.folio.flash = v; }),
      optRange("Wie lange", 1, 5, 0.5, secs, () => s.folio.flashSec, (v) => { s.folio.flashSec = v; }, () => s.folio.flash),
    ];
    case "other": return [el("p", "note", "Apps, die eine Live Activity schicken (z. B. Umwandlungen). Untereinander entscheidet die Priorität, die die App selbst mitschickt.")];
  }
}

// ---------- Rangliste ----------

const list = q<HTMLOListElement>(".rank");
const nameOf = (id: SourceId) => SOURCES[id].name;

function rowEl(id: SourceId, i: number) {
  const n = s.order.length;
  const row = el("li", "row");
  row.dataset.id = id;
  row.style.setProperty("--c", SOURCES[id].color);
  row.classList.toggle("open", open.has(id));
  row.classList.toggle("off", s.hidden.includes(id));

  const main = el("div", "row-main");
  const grip = el("button", "grip");
  grip.append(svg(GRIP));
  grip.title = "Ziehen oder anklicken und ↑ ↓ drücken";
  grip.setAttribute("aria-label", `${nameOf(id)} verschieben, Platz ${i + 1} von ${n}`);
  const num = el("span", "num", String(i + 1));
  const icoBox = el("span", "ico");
  icoBox.append(svg(GLYPH[id]));
  const txt = el("div", "txt");
  txt.append(el("b", "", nameOf(id)), el("small", "st", ""));
  const tags = el("div", "tags");
  const vis = switchEl(!s.hidden.includes(id), `${nameOf(id)} in der kleinen Notch zeigen`, (on) => {
    s.hidden = on ? s.hidden.filter((x) => x !== id) : [...s.hidden, id];
    row.classList.toggle("off", !on);
    announce(on ? `${nameOf(id)} wird wieder gezeigt` : `${nameOf(id)} ist ausgeblendet`);
    commit();
  });
  vis.title = "In der kleinen Notch zeigen";
  const more = el("button", "more");
  more.append(svg(CHEVRON));
  more.setAttribute("aria-label", `Einstellungen für ${nameOf(id)}`);
  more.setAttribute("aria-expanded", String(open.has(id)));
  main.append(grip, num, icoBox, txt, tags, vis, more);

  const ex = el("div", "extras");
  const exIn = el("div", "extras-in");
  const pad = el("div", "extras-pad");
  pad.append(...extras(id));
  exIn.append(pad);
  ex.append(exIn);
  // eingeklappt nicht per Tab erreichbar
  exIn.inert = !open.has(id);
  row.append(main, ex);

  const toggle = () => {
    const on = !open.has(id);
    if (on) open.add(id); else open.delete(id);
    row.classList.toggle("open", on);
    more.setAttribute("aria-expanded", String(on));
    exIn.inert = !on;
  };
  more.addEventListener("click", toggle);
  // grosse Klickflaeche: Symbol und Text klappen ebenfalls auf
  main.addEventListener("click", (e) => {
    if ((e.target as Element).closest(".grip, .switch, .more")) return;
    toggle();
  });

  grip.addEventListener("pointerdown", (e) => startDrag(row, grip, e));
  grip.addEventListener("keydown", (e) => {
    const from = s.order.indexOf(id);
    const to = { ArrowUp: from - 1, ArrowDown: from + 1, Home: 0, End: n - 1 }[e.key];
    if (to == null) return;
    e.preventDefault();
    if (to < 0 || to >= n || to === from) return;
    moveItem(from, to, true);
  });
  return row;
}

function renderList(focusId?: SourceId, flip = false) {
  const before = new Map<string, number>();
  if (flip) for (const r of list.querySelectorAll<HTMLElement>(".row")) before.set(r.dataset.id!, r.getBoundingClientRect().top);
  deps = [];
  list.replaceChildren(...s.order.map((id, i) => rowEl(id, i)));
  syncDeps();
  if (focusId) q<HTMLButtonElement>(`.row[data-id="${focusId}"] .grip`, list)?.focus();
  if (flip) {
    // FLIP: Zeilen gleiten an ihren neuen Platz statt zu springen
    const rows = [...list.querySelectorAll<HTMLElement>(".row")];
    for (const r of rows) {
      const b = before.get(r.dataset.id!);
      const d = b == null ? 0 : b - r.getBoundingClientRect().top;
      if (d) { r.style.transition = "none"; r.style.transform = `translateY(${d}px)`; }
    }
    void list.offsetWidth;
    for (const r of rows) { r.style.transition = ""; r.style.transform = ""; }
  }
  refresh();
}

function moveItem(from: number, to: number, flip = false) {
  const [id] = s.order.splice(from, 1);
  s.order.splice(to, 0, id);
  commit();
  renderList(id, flip);
  announce(`${nameOf(id)} jetzt auf Platz ${to + 1}`);
}

/** Ziehen am Griff: Zeile folgt der Maus, die anderen machen Platz; Loslassen setzt sie dort ab. */
function startDrag(row: HTMLElement, grip: HTMLElement, e: PointerEvent) {
  if (e.button !== 0) return;
  e.preventDefault();
  const rows = [...list.querySelectorAll<HTMLElement>(".row")];
  const i0 = rows.indexOf(row);
  const rects = rows.map((r) => r.getBoundingClientRect());
  const gap = parseFloat(getComputedStyle(list).rowGap) || 6;
  const h = rects[i0].height + gap;
  const minDy = rects[0].top - rects[i0].top;
  const maxDy = rects[rects.length - 1].bottom - rects[i0].bottom;
  const y0 = e.clientY;
  let target = i0;
  let moved = false;
  grip.setPointerCapture(e.pointerId);

  const move = (ev: PointerEvent) => {
    const dy = Math.max(minDy, Math.min(maxDy, ev.clientY - y0));
    if (!moved && Math.abs(ev.clientY - y0) < 4) return;
    if (!moved) { moved = true; list.classList.add("sorting"); row.classList.add("dragging"); }
    row.style.transform = `translateY(${dy}px)`;
    const center = rects[i0].top + rects[i0].height / 2 + dy;
    target = rects.filter((r, j) => j !== i0 && r.top + r.height / 2 < center).length;
    rows.forEach((r, j) => {
      if (j === i0) return;
      const sh = j > i0 && j <= target ? -h : j < i0 && j >= target ? h : 0;
      r.style.transform = sh ? `translateY(${sh}px)` : "";
    });
  };
  const end = () => {
    grip.removeEventListener("pointermove", move);
    grip.removeEventListener("pointerup", end);
    grip.removeEventListener("pointercancel", end);
    if (!moved) { grip.focus(); return; } // nur angeklickt: jetzt geht es mit ↑ ↓ weiter
    const dy = target > i0 ? rects[target].bottom - rects[i0].bottom : target < i0 ? rects[target].top - rects[i0].top : 0;
    row.style.transition = "transform 0.18s var(--settle)";
    row.style.transform = `translateY(${dy}px)`;
    window.setTimeout(() => {
      list.classList.remove("sorting");
      if (target !== i0) moveItem(i0, target);
      else { row.classList.remove("dragging"); row.style.transition = ""; row.style.transform = ""; }
    }, 180);
  };
  grip.addEventListener("pointermove", move);
  grip.addEventListener("pointerup", end);
  grip.addEventListener("pointercancel", end);
}

// ---------- Segment-Schalter, Ausprobieren ----------

function syncSegs() {
  document.querySelectorAll<HTMLButtonElement>("[data-slots]").forEach((b) => b.setAttribute("aria-checked", String(Number(b.dataset.slots) === s.slots)));
  document.querySelectorAll<HTMLButtonElement>("[data-mode]").forEach((b) => b.setAttribute("aria-checked", String(b.dataset.mode === mode)));
  q(".slots-hint").textContent = s.slots === 2
    ? "Zwei Dinge nebeneinander – z. B. das Cover links und der Blutzucker rechts."
    : "Nur das Wichtigste. Ruhiger, dafür steht immer nur eine Sache da.";
  q(".demo").hidden = mode !== "demo";
}

function demoChips() {
  const box = q(".demo-chips");
  box.replaceChildren(...(Object.keys(SOURCES) as SourceId[]).map((id) => {
    const b = el("button", "dchip");
    b.style.setProperty("--c", SOURCES[id].color);
    b.append(svg(GLYPH[id]), document.createTextNode(SOURCES[id].name));
    b.setAttribute("aria-pressed", String(demoOn.has(id)));
    b.addEventListener("click", () => {
      if (demoOn.has(id)) demoOn.delete(id); else demoOn.add(id);
      b.setAttribute("aria-pressed", String(demoOn.has(id)));
      refresh();
    });
    return b;
  }));
}

function renderAll() {
  syncSegs();
  renderList();
}

async function main() {
  s = normalize(await invoke<unknown>("settings_get").catch(() => null));

  // aendert jemand anderes (oder die Notch) die Einstellungen, hier uebernehmen
  await listen<unknown>("settings", (e) => {
    const n = normalize(e.payload);
    if (JSON.stringify(n) === JSON.stringify(s)) return;
    s = n;
    renderAll();
  });

  // Live-Daten wie in der Notch
  await listen<Media>("media", (e) => {
    if (liveMedia?.playing && e.payload && !e.payload.playing) pausedAt = Date.now();
    liveMedia = e.payload;
    refresh();
  });
  await listen<{ src: string }>("media-cover", (e) => { liveCover = e.payload.src; refresh(); });
  await listen<CAct[]>("activities", (e) => { liveActs = e.payload; refresh(); });
  const snap = await invoke<{ media: Media; cover: string | null; activities: CAct[] }>("snapshot").catch(() => null);
  if (snap) { liveMedia = snap.media; liveCover = snap.cover; liveActs = snap.activities; }

  // ohne laufende Quellen gleich zum Ausprobieren
  const c0 = current();
  if (!c0.plan.slots.length && !c0.plan.timer) mode = "demo";

  document.querySelectorAll<HTMLButtonElement>("[data-slots]").forEach((b) =>
    b.addEventListener("click", () => { s.slots = Number(b.dataset.slots) === 1 ? 1 : 2; syncSegs(); commit(); }));
  document.querySelectorAll<HTMLButtonElement>("[data-mode]").forEach((b) =>
    b.addEventListener("click", () => { mode = b.dataset.mode === "demo" ? "demo" : "live"; pvSig = ""; syncSegs(); refresh(); }));

  const bpm = q<HTMLInputElement>(".d-bpm"), bpmOut = q(".d-bpm-out");
  const bg = q<HTMLInputElement>(".d-bg"), bgOut = q(".d-bg-out");
  const syncDemo = () => {
    demoBpm = Number(bpm.value); demoBg = Number(bg.value);
    bpmOut.textContent = `${demoBpm} bpm`;
    bgOut.textContent = `${demoBg} mg/dL`;
    setFill(bpm); setFill(bg);
  };
  bpm.addEventListener("input", () => { syncDemo(); refresh(); });
  bg.addEventListener("input", () => { syncDemo(); refresh(); });
  syncDemo();
  q(".d-flip").addEventListener("click", () => {
    demoOn.add("folio");
    demoChips();
    demoPage++;
    const ms = s.folio.flash ? s.folio.flashSec * 1000 : 0;
    demoFlashUntil = Date.now() + ms;
    refresh();
    if (ms) window.setTimeout(refresh, ms + 30);
  });
  q(".reset").addEventListener("click", () => {
    s = clone(normalize(DEFAULTS));
    open.clear();
    commit();
    renderAll();
    announce("Auf Standard zurückgesetzt");
  });

  demoChips();
  renderAll();

  // Beispiel-Timer laeuft rueckwaerts; Musik-Nachlauf und veraltete Werte brauchen ab und zu einen Blick
  setInterval(() => {
    if (mode === "demo" && demoOn.has("timer")) demoTimerLeft = demoTimerLeft > 1 ? demoTimerLeft - 1 : 600;
    refresh();
  }, 1000);
}

main();
