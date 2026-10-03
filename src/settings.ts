/**
 * Einstellungsfenster der Notch (eigenes Fenster, nicht in der Notch).
 * Rangliste der kleinen Notch: ziehen am Griff oder Griff + Pfeiltasten, Schalter = ausblenden,
 * Pfeil = Extras der Quelle. Oben eine Vorschau, die dieselbe Logik benutzt wie die Notch
 * (compact.ts) — live mit echten Daten oder zum Ausprobieren mit Beispielwerten.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { build, bpmOf, needed, plan, planKey, PulseGate, sourceOf, TIMER_ID, type CAct, type Plan } from "./compact";
import { glassLight, icon, iconNode, iconUrl, lightScroller, liquid, segments, windowControls, type IconName } from "./nojo/nojo-ui";
import { clone, DEFAULTS, FULLSCREEN_MODES, LAYERS, MUSIC_REACT, normalize, SOURCES, SPORT_EXPAND, type CompactSettings, type FullscreenMode, type Layer, type MusicReact, type SourceId, type SportExpand } from "./settings-model";

const q = <T extends Element = HTMLElement>(s: string, root: ParentNode = document) => root.querySelector(s) as T;
function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
}
/** Symbol aus der Bibliothek als Element */
const svg = (name: IconName) => iconNode(name);

/** Symbol je Quelle — dieselben wie ueberall im Oekosystem (Herz = Puls, Tropfen = Blutzucker …) */
const GLYPH: Record<SourceId, IconName> = { glucose: "drop", music: "music", timer: "timer-fill", pulse: "heart", folio: "document", other: "apps" };
const GRIP: IconName = "grip";

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
const ico = (name: IconName, c: string) => iconUrl(name, c);

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
    case "pulse": return a ? `${bpmOf(a)} bpm · ${a.app === "Haze" ? "Garmin über Haze" : a.app}` : "Kein Pulsmesser verbunden";
    case "folio": return a ? `${a.title} · Seite ${a.value ?? "?"} ${a.unit ?? ""}`.trim() : "Kein Dokument offen";
    case "other": {
      const o = c.acts.filter((x) => sourceOf(x) === "other");
      return o.length ? o.map((x) => x.title).join(", ") : "Gerade nichts";
    }
  }
}

/** Markierungen als Status-Pillen wie in Haze; "up" in der Warnfarbe */
function tag(cls: string, text: string, title = "") {
  const t = el("span", `status-pill ${cls === "up" ? "up" : ""}`, text);
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
  updateRows(c);
}

// ---------- Bausteine fuer die Extras ----------

/** Abhaengige Optionen (z. B. Schwelle nur, wenn der Schalter an ist) und Texte, die mitlaufen */
let deps: (() => void)[] = [];
const syncDeps = () => deps.forEach((f) => f());

/** Schalter wie in Haze (Pille, an = hell mit dunklem Knopf) — ohne Zeile, z. B. in der Ranglisten-Zeile */
function switchEl(checked: boolean, label: string, onChange: (on: boolean) => void) {
  const l = el("label", "vis");
  const inp = el("input");
  inp.type = "checkbox";
  inp.checked = checked;
  inp.setAttribute("role", "switch");
  inp.setAttribute("aria-label", label);
  l.append(inp, el("span", "toggle"));
  inp.addEventListener("change", () => onChange(inp.checked));
  return l;
}

/** Haze-Toggle-Zeile: Text links, Schalter rechts */
function optToggle(label: string, get: () => boolean, set: (v: boolean) => void) {
  const row = el("label", "toggle-row");
  const text = el("span", "", label);
  const inp = el("input");
  inp.type = "checkbox";
  inp.checked = get();
  inp.setAttribute("role", "switch");
  inp.addEventListener("change", () => { set(inp.checked); syncDeps(); commit(); });
  row.append(text, inp, el("span", "toggle"));
  return row;
}

/** Haze-Feld mit Regler: „Name · Wert“, daneben „Standard“ */
function optRange(
  label: string, min: number, max: number, step: number, def: number, fmt: (v: number) => string,
  get: () => number, set: (v: number) => void, enabled?: () => boolean, sub?: () => string,
) {
  const o = el("div", "field");
  const head = el("span", "", `${label} · ${fmt(get())}`);
  const rr = el("div", "range-row");
  const r = el("input");
  r.type = "range";
  r.min = String(min); r.max = String(max); r.step = String(step); r.value = String(get());
  r.setAttribute("aria-label", label);
  const std = el("button", "link-button", "Standard");
  std.disabled = get() === def;
  rr.append(r, std);
  o.append(head, rr);
  let small: HTMLElement | null = null;
  if (sub) { small = el("small", "", sub()); o.append(small); }
  const apply = (v: number, now: boolean) => {
    set(v);
    head.textContent = `${label} · ${fmt(v)}`;
    std.disabled = v === def;
    syncDeps();
    commit(now);
  };
  r.addEventListener("input", () => apply(Number(r.value), false));
  r.addEventListener("change", () => commit());
  std.addEventListener("click", () => { r.value = String(def); apply(def, true); });
  deps.push(() => {
    if (enabled) o.classList.toggle("dim", !enabled());
    if (small && sub) small.textContent = sub();
  });
  return o;
}

const secs = (v: number) => (v === 0 ? "sofort weg" : `${String(v).replace(".", ",")} s`);

const REACT_TEXT: Record<MusicReact, string> = { off: "Aus", auto: "Auto", manual: "Eigene" };
const LAYER_NAME: Record<Layer, string> = { spikes: "Zacken", wave: "Welle", pulse: "Puls" };

/** Musik-Reaktion: Aus / Auto / Eigene, bei Eigene die Stile frei kombinierbar; darunter live, was erkannt wird */
function reactField() {
  const field = el("div", "field");
  field.setAttribute("role", "group");
  field.setAttribute("aria-label", "Notch reagiert auf Musik");
  field.append("Notch reagiert auf Musik");
  const seg = el("div", "segmented compact");
  for (const m of MUSIC_REACT) {
    const b = el("button", "", REACT_TEXT[m]);
    b.dataset.react = m;
    b.addEventListener("click", () => { s.music.react = m; syncDeps(); commit(); syncReact(); });
    seg.append(b);
  }
  // Eigene: mehrere gleichzeitig (sie ueberlagern sich), mindestens einer bleibt an
  const mix = el("div", "segmented compact react-layers");
  for (const l of LAYERS) {
    const b = el("button", "", LAYER_NAME[l]);
    b.dataset.layer = l;
    b.addEventListener("click", () => {
      const next = { ...s.music.layers, [l]: !s.music.layers[l] };
      if (!next.spikes && !next.wave && !next.pulse) return;
      s.music.layers = next;
      commit();
      syncReact();
    });
    mix.append(b);
  }
  const live = el("small", "react-live", reactHint());
  live.setAttribute("aria-live", "polite");
  field.append(seg, mix, live);
  queueMicrotask(syncReact);
  return field;
}

function syncReact() {
  setSeg("button[data-react]", (b) => b.dataset.react === s.music.react);
  setSeg("button[data-layer]", (b) => s.music.layers[b.dataset.layer as Layer]);
  document.querySelectorAll<HTMLElement>(".react-layers").forEach((e) => { e.hidden = s.music.react !== "manual"; });
  showReactHint();
}

let reactLast: { s: string; bpm: number; beat: number; w?: number[]; at: number } | null = null;
const showReactHint = () => document.querySelectorAll(".react-live").forEach((e) => { e.textContent = reactHint(); });
function reactHint() {
  if (s.music.react === "off") return "Die schwarze Notch bewegt sich an allen drei Seiten zur Musik: Zacken für harte, schnelle Musik, Welle für ruhige, Puls für alles dazwischen. „Auto“ mischt die drei selbst, bei „Eigene“ wählst du sie.";
  if (!reactLast || Date.now() - reactLast.at > 1500) return "Wartet auf Musik …";
  const tempo = reactLast.bpm ? ` · ${reactLast.bpm} BPM · Takt ${reactLast.beat} %` : " · Tempo wird ermittelt …";
  const w = reactLast.w ?? [0, 0, 100];
  const mix = LAYERS.map((l, i) => `${LAYER_NAME[l]} ${w[i]} %`).join(" · ");
  return s.music.react === "auto" ? `Erkannt: ${mix}${tempo}` : `Musik: ${LAYER_NAME[reactLast.s as Layer] ?? reactLast.s}-artig${tempo}`;
}

function extras(id: SourceId): HTMLElement[] {
  switch (id) {
    case "glucose": return [
      optToggle("Außerhalb des Zielbereichs ganz nach oben", () => s.glucose.outOfRangeTop, (v) => { s.glucose.outOfRangeTop = v; }),
      optToggle("Änderung anzeigen", () => s.glucose.delta, (v) => { s.glucose.delta = v; }),
    ];
    case "music": return [
      optToggle("Pegel neben das Cover statt ausblenden", () => s.music.eqBesideCover, (v) => { s.music.eqBesideCover = v; }),
      optRange("Nach Pause noch zeigen", 0, 120, 5, DEFAULTS.music.lingerSec, secs, () => s.music.lingerSec, (v) => { s.music.lingerSec = v; }),
      reactField(),
      optRange("Stärke", 0.5, 2, 0.1, DEFAULTS.music.strength, (v) => `${Math.round(v * 100)} %`, () => s.music.strength, (v) => { s.music.strength = v; }, () => s.music.react !== "off"),
    ];
    case "timer": return [
      optToggle("Notch verbreitern statt Platz nehmen", () => s.timer.expand, (v) => { s.timer.expand = v; }),
    ];
    case "pulse": return [
      optToggle("Bei hohem Puls nach oben", () => s.pulse.boost, (v) => { s.pulse.boost = v; }),
      optRange("Ab", 90, 200, 5, DEFAULTS.pulse.threshold, (v) => `${v} bpm`, () => s.pulse.threshold, (v) => { s.pulse.threshold = v; }, () => s.pulse.boost),
    ];
    case "folio": return [
      optToggle("Seitenzahl beim Blättern einblenden", () => s.folio.flash, (v) => { s.folio.flash = v; }),
      optRange("Wie lange", 1, 5, 0.5, DEFAULTS.folio.flashSec, secs, () => s.folio.flashSec, (v) => { s.folio.flashSec = v; }, () => s.folio.flash),
    ];
    case "other": return [];
  }
}

// ---------- Rangliste ----------

const list = q<HTMLOListElement>(".rank");
const nameOf = (id: SourceId) => SOURCES[id].name;

function rowEl(id: SourceId, i: number) {
  const n = s.order.length;
  // jede Quelle ist eine aufklappbare Gruppe wie in den Haze-Einstellungen
  const row = el("li", "row settings-group");
  row.dataset.id = id;
  row.classList.toggle("open", open.has(id));
  row.classList.toggle("off", s.hidden.includes(id));

  const main = el("div", "row-main");
  const grip = el("button", "grip");
  grip.append(svg(GRIP));
  grip.title = "Ziehen oder anklicken und ↑ ↓ drücken";
  grip.setAttribute("aria-label", `${nameOf(id)} verschieben, Platz ${i + 1} von ${n}`);
  const icoBox = el("span", "ico");
  icoBox.append(svg(GLYPH[id]), el("span", "num", String(i + 1)));
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
  more.append(el("span", "chev"));
  more.setAttribute("aria-label", `Einstellungen für ${nameOf(id)}`);
  more.setAttribute("aria-expanded", String(open.has(id)));
  main.append(grip, icoBox, txt, tags, vis, more);

  const ex = el("div", "extras");
  const exIn = el("div", "extras-in");
  const pad = el("div", "extras-pad");
  const items = extras(id);
  pad.append(...items);
  // ohne Extras (Andere Apps) gibt es nichts aufzuklappen
  more.style.visibility = items.length ? "" : "hidden";
  exIn.append(pad);
  ex.append(exIn);
  // eingeklappt nicht per Tab erreichbar
  exIn.inert = !open.has(id);
  row.append(main, ex);

  const toggle = () => {
    if (!items.length) return;
    const on = !open.has(id);
    if (on) open.add(id); else open.delete(id);
    row.classList.toggle("open", on);
    more.setAttribute("aria-expanded", String(on));
    exIn.inert = !on;
  };
  more.addEventListener("click", toggle);
  // grosse Klickflaeche: Symbol und Text klappen ebenfalls auf
  main.addEventListener("click", (e) => {
    if ((e.target as Element).closest(".grip, .vis, .more")) return;
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

/** Segmente wie in Haze: aktiver Knopf hell (Klasse active + aria-pressed) */
function setSeg(sel: string, on: (b: HTMLButtonElement) => boolean) {
  document.querySelectorAll<HTMLButtonElement>(sel).forEach((b) => {
    const a = on(b);
    b.classList.toggle("active", a);
    b.setAttribute("aria-pressed", String(a));
  });
}

let dock = "top";

function syncSegs() {
  setSeg("[data-slots]", (b) => Number(b.dataset.slots) === s.slots);
  setSeg("[data-mode]", (b) => b.dataset.mode === mode);
  setSeg("button[data-dock]", (b) => b.dataset.dock === dock);
  q(".demo").hidden = mode !== "demo";
}

// ---------- Reiter (wie Haze: Unterstrich unter dem aktiven) ----------

function setTab(t: string) {
  document.querySelectorAll<HTMLButtonElement>("[data-tab]").forEach((b) => b.setAttribute("aria-selected", String(b.dataset.tab === t)));
  document.querySelectorAll<HTMLElement>("[data-pane]").forEach((p) => { p.hidden = p.dataset.pane !== t; });
  q(".panel-body").scrollTop = 0;
  try { localStorage.setItem("settings-tab", t); } catch { /* egal */ }
}

function demoChips() {
  const box = q(".demo-chips");
  box.replaceChildren(...(Object.keys(SOURCES) as SourceId[]).map((id) => {
    const b = el("button");
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

const FULLSCREEN_TEXT: Record<FullscreenMode, [string, string]> = {
  hide: ["Ausblenden", "Läuft ein Programm im Vollbild, fährt die Notch weg."],
  peek: ["Am Rand", "Die Notch fährt weg. Fährst du mit der Maus an die Bildschirmkante, wo sie sonst sitzt, kommt sie heraus – und verschwindet wieder, sobald die Maus weggeht. Mit Xbox-Controller: Steuerkreuz links + RB holt sie klein heraus, Steuerkreuz links + RT aufgeklappt; nochmal drücken oder nach 10 s wieder weg."],
  show: ["Nur anzeigen", "Die Notch bleibt klein sichtbar (z. B. Blutzucker beim Spielen), ist aber nicht anklickbar: Klicks gehen durch sie hindurch ans Programm, und sie klappt nicht auf."],
};

function renderDock() {
  const field = el("div", "field");
  field.setAttribute("role", "group");
  field.setAttribute("aria-label", "Im Vollbild");
  field.append("Im Vollbild");
  const seg = el("div", "segmented compact");
  for (const m of FULLSCREEN_MODES) {
    const b = el("button", "", FULLSCREEN_TEXT[m][0]);
    b.dataset.fsmode = m;
    b.addEventListener("click", () => { s.fullscreen.mode = m; commit(); renderDock(); });
    seg.append(b);
  }
  field.append(seg, el("small", "", FULLSCREEN_TEXT[s.fullscreen.mode][1]));
  q(".dock-options").replaceChildren(field);
  setSeg("button[data-fsmode]", (b) => b.dataset.fsmode === s.fullscreen.mode);
}

// ---------- Live-Sport ----------

type LeagueInfo = { id: string; name: string; group: string; sport: string; source: string };
type TeamInfo = { key: string; name: string; logo: string };
type LiveTeam = { name: string; abbr?: string; logo?: string; color?: string; score: string };
type LiveMatch = { key?: string; state: string; fav: boolean; league_name: string; home: LiveTeam; away: LiveTeam; clock: string; start?: number };
type SportLive = { matches: LiveMatch[]; error?: string; updated?: number; off?: boolean };
let leagues: LeagueInfo[] = [];
let sportLive: SportLive | null = null;
/** Teamsuche: Wettbewerb, aus dem die Liste kommt, und geladene Mannschaften */
let teamLeague = "";
let teamPicking = false;
/** Arena (eigene Sport-App) installiert? */
let arenaThere = false;
const teamCache = new Map<string, TeamInfo[]>();

/** kurze Erklaerung nur zur gewaehlten Stufe */
const EXPAND_TEXT: Record<SportExpand, [string, string]> = {
  goals: ["Toren", "Tore und Rote Karten"],
  important: ["Wichtigem", "dazu Anpfiff, Halbzeit, Abpfiff, Elfmeter, Videobeweis"],
  all: ["Allem", "dazu Gelbe Karten, Wechsel, Pfosten"],
  off: ["Nie", "nur der Spielstand in der Mitte"],
};

/** Wappen statt Vereinsname; der Name steht im Tooltip (und fuer Screenreader) */
function crest(t: { name: string; logo?: string; abbr?: string; color?: string }, cls = "crest") {
  const c = el("span", cls);
  c.title = t.name;
  c.setAttribute("role", "img");
  c.setAttribute("aria-label", t.name);
  const mono = () => {
    c.classList.add("mono");
    c.textContent = (t.abbr || t.name.replace(/^(1\.|FC|SV|VfB|VfL|TSG|SC|FSV|SpVgg)\s+/i, "")).slice(0, 3).toUpperCase();
    if (t.color) c.style.setProperty("--c", t.color);
  };
  if (t.logo) {
    const i = new Image();
    i.decoding = "async";
    i.alt = "";
    i.src = t.logo;
    i.onerror = () => { i.remove(); mono(); };
    c.append(i);
  } else mono();
  return c;
}

const clockOf = (m: LiveMatch) => {
  if (m.state === "pre" && m.start) {
    const d = new Date(m.start);
    const t = d.toLocaleTimeString("de-DE", { hour: "2-digit", minute: "2-digit" });
    return new Date().toDateString() === d.toDateString() ? t : `${d.toLocaleDateString("de-DE", { weekday: "short" }).replace(".", "")} ${t}`;
  }
  return m.clock || (m.state === "post" ? "Ende" : "");
};

/** Spielstand-Kaertchen: Wappen  2:1  Wappen  67' */
function scoreBug(m: LiveMatch) {
  const b = el("span", `bug ${m.state}`);
  b.title = `${m.home.name} – ${m.away.name} · ${m.league_name}`;
  b.append(crest(m.home), el("b", "", m.state === "pre" ? "–" : `${m.home.score || 0}:${m.away.score || 0}`), crest(m.away), el("small", "", clockOf(m)));
  return b;
}

/** Leiste oben: was gerade live laeuft (oder wann das naechste Spiel ist) */
function renderSportLive() {
  const box = q(".sport-live");
  if (!box) return;
  box.hidden = !s.sport.on;
  if (!s.sport.on) return;
  const parts: HTMLElement[] = [];
  if (!sportLive) parts.push(el("span", "faint", "Wird geladen …"));
  else if (sportLive.error) parts.push(el("span", "faint", sportLive.error));
  else {
    const live = sportLive.matches.filter((m) => m.state === "in");
    const next = sportLive.matches.filter((m) => m.state === "pre").sort((x, y) => (x.start ?? 0) - (y.start ?? 0));
    if (live.length) {
      parts.push(el("span", "live-dot", "Live"));
      for (const m of [...live].sort((x, y) => Number(y.fav) - Number(x.fav)).slice(0, 3)) parts.push(scoreBug(m));
      if (live.length > 3) parts.push(el("span", "faint", `+${live.length - 3}`));
    } else if (next.length) {
      parts.push(el("span", "faint", "Als Nächstes"), scoreBug(next[0]));
    } else parts.push(el("span", "faint", "Gerade kein Spiel"));
  }
  box.replaceChildren(...parts);
}

function card(title: string) {
  const c = el("section", "s-card");
  const h = el("div", "s-head");
  h.append(el("span", "eyebrow", title));
  c.append(h);
  return c;
}

function renderSport() {
  const box = q(".sport-options");
  if (!box) return;
  const parts: HTMLElement[] = [];
  const on = optToggle("Live-Sport", () => s.sport.on, (v) => { s.sport.on = v; queueMicrotask(renderSport); });
  on.querySelector("span")!.append(el("small", "", "Spielstand, Tore und Spielfeld in der Notch"));
  parts.push(on);
  const live = el("div", "sport-live n-glass n-liquid");
  live.setAttribute("aria-live", "polite");
  parts.push(live);
  if (arenaThere) {
    const a = el("button", "arena-link n-card");
    a.append(el("b", "", "Arena öffnen"), el("span", "", "Spielplan, Tabellen, Spielfeld groß – gleiche Teams und Wettbewerbe"));
    a.addEventListener("click", () => invoke("open", { target: "arena://start" }).catch(() => {}));
    parts.push(a);
  }

  const rest = el("div", "sport-rest");
  rest.classList.toggle("dim", !s.sport.on);
  rest.inert = !s.sport.on;

  // Deine Teams: nur Wappen; „Hinzufügen“ oeffnet die Auswahl
  const teams = card("Deine Teams");
  const picking = teamPicking || !s.sport.teams.length;
  const add = el("button", "s-act", picking ? "Fertig" : "Hinzufügen");
  add.hidden = !s.sport.teams.length;
  add.addEventListener("click", () => { teamPicking = !teamPicking; renderSport(); });
  teams.querySelector(".s-head")!.append(add);
  if (s.sport.teams.length) {
    const mine = el("div", "crest-row");
    for (const t of s.sport.teams) {
      const b = el("button", "crest-tile n-liquid");
      b.title = `${t.name} – entfernen`;
      b.setAttribute("aria-label", `${t.name} entfernen`);
      b.append(crest(t), el("i", "x", "×"));
      b.addEventListener("click", () => { s.sport.teams = s.sport.teams.filter((x) => x.key !== t.key); commit(); announce(`${t.name} entfernt`); renderSport(); });
      mine.append(b);
    }
    teams.append(mine);
  }

  if (picking) {
    if (!s.sport.teams.length) teams.append(el("p", "s-hint", "Wappen antippen – Spiele deiner Teams stehen dann vorn und in der Mitte."));
    const pick = el("div", "team-pick");
    const sel = el("select", "text-input team-league") as HTMLSelectElement;
    for (const l of leagues) {
      const o = el("option", "", l.name) as HTMLOptionElement;
      o.value = l.id;
      sel.append(o);
    }
    if (!teamLeague) teamLeague = s.sport.leagues[0] ?? leagues[0]?.id ?? "bl1";
    sel.value = teamLeague;
    const find = el("input", "text-input team-find") as HTMLInputElement;
    find.type = "search";
    find.placeholder = "Suchen";
    find.spellcheck = false;
    const results = el("div", "crest-grid team-results");
    const showTeams = () => {
      const list = teamCache.get(teamLeague);
      results.replaceChildren();
      if (!list) { results.append(el("small", "faint", "Wird geladen …")); return; }
      const qx = find.value.trim().toLowerCase();
      const hits = list.filter((t) => !s.sport.teams.some((x) => x.key === t.key) && (!qx || t.name.toLowerCase().includes(qx)));
      if (!hits.length) results.append(el("small", "faint", list.length ? "Kein Treffer" : "Keine Mannschaften"));
      for (const t of hits) {
        const b = el("button", "crest-pick");
        b.title = t.name;
        b.setAttribute("aria-label", `${t.name} hinzufügen`);
        b.append(crest(t));
        b.addEventListener("click", () => {
          s.sport.teams = [...s.sport.teams, { key: t.key, name: t.name, ...(t.logo ? { logo: t.logo } : {}) }];
          teamPicking = true;
          commit();
          announce(`${t.name} hinzugefügt`);
          renderSport();
          q<HTMLInputElement>(".team-find")?.focus();
        });
        results.append(b);
      }
    };
    const load = () => {
      showTeams();
      if (teamCache.has(teamLeague)) return;
      const want = teamLeague;
      invoke<TeamInfo[]>("sport_teams", { league: want })
        .then((t) => { teamCache.set(want, t); if (teamLeague === want) showTeams(); })
        .catch((e) => { if (teamLeague === want) results.replaceChildren(el("small", "faint", `Nicht erreichbar: ${e}`)); });
    };
    sel.addEventListener("change", () => { teamLeague = sel.value; load(); });
    find.addEventListener("input", showTeams);
    pick.append(sel, find);
    teams.append(pick, results);
    if (leagues.length) queueMicrotask(load);
  }
  rest.append(teams);

  // Wettbewerbe
  const comp = card("Wettbewerbe");
  for (const g of [...new Set(leagues.map((l) => l.group))]) {
    const chips = el("div", "chips");
    for (const l of leagues.filter((x) => x.group === g)) {
      const b = el("button", "", l.name);
      b.setAttribute("aria-pressed", String(s.sport.leagues.includes(l.id)));
      b.addEventListener("click", () => {
        s.sport.leagues = s.sport.leagues.includes(l.id) ? s.sport.leagues.filter((x) => x !== l.id) : [...s.sport.leagues, l.id];
        b.setAttribute("aria-pressed", String(s.sport.leagues.includes(l.id)));
        commit();
      });
      chips.append(b);
    }
    comp.append(el("small", "chip-group", g), chips);
  }
  if (!leagues.length) comp.append(el("small", "faint", "Wird geladen …"));
  rest.append(comp);

  // In der Notch
  const notch = card("In der Notch");
  const seg = el("div", "segmented compact");
  for (const [v, label] of [["all", "Alle Spiele"], ["fav", "Nur meine Teams"]] as const) {
    const b = el("button", "", label);
    b.dataset.scope = v;
    b.addEventListener("click", () => { s.sport.scope = v; commit(); setSeg("button[data-scope]", (x) => x.dataset.scope === s.sport.scope); });
    seg.append(b);
  }
  const ex = el("div", "field");
  const exHead = el("span", "", "Aufklappen bei");
  const exHint = el("small", "", EXPAND_TEXT[s.sport.expand][1]);
  const exSeg = el("div", "segmented compact");
  for (const v of SPORT_EXPAND) {
    const b = el("button", "", EXPAND_TEXT[v][0]);
    b.dataset.expand = v;
    b.addEventListener("click", () => {
      s.sport.expand = v;
      exHint.textContent = EXPAND_TEXT[v][1];
      commit();
      setSeg("button[data-expand]", (x) => x.dataset.expand === s.sport.expand);
    });
    exSeg.append(b);
  }
  ex.append(exHead, exSeg, exHint);
  const center = optToggle("Spielstand in der Mitte", () => s.sport.center, (v) => { s.sport.center = v; });
  const pitch = optToggle("Spielfeld mit Ballverlauf", () => s.sport.pitch, (v) => { s.sport.pitch = v; });
  pitch.title = "Lädt nur, solange das Spielfeld zu sehen ist (etwa 10–15 MB pro Stunde).";
  const full = optToggle("Tore auch im Vollbild", () => s.sport.fullscreen, (v) => { s.sport.fullscreen = v; });
  full.title = "Nur wenn die Notch im Vollbild am Rand bleibt (Reiter Andocken).";
  notch.append(seg, ex, center, pitch, full);
  rest.append(notch);

  const src = el("details", "fine-more");
  src.append(el("summary", "", "Daten: ESPN · OpenLigaDB"),
    el("p", "", "Beide frei abrufbar, ohne Konto. ESPN ist inoffiziell und kann sich ändern; OpenLigaDB wird von der Community gepflegt (3. Liga, Frauen-Bundesliga, Ersatz für Bundesliga und Pokal). Echte Positionen aller Spieler gibt es live nirgends frei – das Spielfeld zeigt den Ballverlauf mit Rückennummern."));
  rest.append(src);

  parts.push(rest);
  box.replaceChildren(...parts);
  renderSportLive();
  setSeg("button[data-scope]", (x) => x.dataset.scope === s.sport.scope);
  setSeg("button[data-expand]", (x) => x.dataset.expand === s.sport.expand);
}

// ---------- iPhone (share.rs) ----------

type ShareDev = { fp: string; alias: string; model: string; kind: string; recent: boolean };
type ShareState = { on: boolean; running: boolean; error: string; ip: string; port: number; alias: string; folder: string; devices: ShareDev[] };
let share: ShareState | null = null;

const ICON_PHONE = icon("phone");
const ICON_QR = icon("qr");
const ICON_FOLDER = icon("folder");

/** Zustand in wenigen Worten; leuchtet, wenn bereit */
function shareStatus(): [string, string] {
  if (!s.share.on) return ["off", "Aus"];
  if (!share) return ["wait", "Startet …"];
  if (share.error && !share.running) return ["err", share.error];
  if (!share.running) return ["wait", "Startet …"];
  if (!share.ip) return ["err", "Kein WLAN gefunden"];
  return ["ok", `Bereit als „${share.alias}“`];
}

function renderShareStatus() {
  const st = q(".share-status");
  if (!st) return;
  const [k, t] = shareStatus();
  st.className = `share-status n-glass n-liquid ${k}`;
  st.replaceChildren(el("i", "dot"), t);
  st.title = share?.ip ? `${share.ip}:${share.port}${share.error ? ` · ${share.error}` : ""}` : "";
}

function renderShare() {
  const box = q(".share-options");
  if (!box) return;
  const parts: HTMLElement[] = [];
  const on = optToggle("iPhone-Austausch", () => s.share.on, (v) => { s.share.on = v; queueMicrotask(renderShare); });
  on.querySelector("span")!.append(el("small", "", "Wie AirDrop – über dein WLAN, ohne Cloud"));
  parts.push(on);
  const st = el("div", "share-status");
  st.setAttribute("aria-live", "polite");
  parts.push(st);

  const rest = el("div", "sport-rest");
  rest.classList.toggle("dim", !s.share.on);
  rest.inert = !s.share.on;

  // Verbinden: zwei Wege nebeneinander
  const ways = el("div", "ways");
  const ls = el("section", "s-card way");
  const lsHead = el("div", "way-head");
  lsHead.innerHTML = ICON_PHONE;
  lsHead.append(el("b", "", "LocalSend"));
  const devs = el("div", "devs");
  const near = (share?.devices ?? []).filter((d) => d.recent);
  if (!near.length) devs.append(el("small", "faint", "Kein Gerät in der Nähe"));
  for (const d of near) {
    const p = el("span", "dev", d.alias);
    p.title = d.model || d.alias;
    devs.append(p);
  }
  ls.append(lsHead, devs);

  const qr = el("section", "s-card way");
  const qrHead = el("div", "way-head");
  qrHead.innerHTML = ICON_QR;
  qrHead.append(el("b", "", "Ohne App"));
  const qrBox = el("div", "qr-box");
  const qrBtn = el("button", "button secondary", "QR-Code zeigen");
  qrBtn.addEventListener("click", async () => {
    try {
      const r = await invoke<{ url: string; svg: string; minutes: number }>("share_qr");
      const img = new Image();
      img.src = `data:image/svg+xml;utf8,${encodeURIComponent(r.svg)}`;
      img.alt = "QR-Code für die iPhone-Kamera";
      img.className = "qr";
      img.title = r.url;
      qrBox.replaceChildren(img, el("small", "faint", `Mit der Kamera scannen · ${r.minutes} Min gültig`));
      qrBtn.hidden = true;
    } catch (e) {
      qrBox.replaceChildren(el("small", "error", String(e)));
    }
  });
  qr.append(qrHead, qrBtn, qrBox);
  ways.append(ls, qr);
  rest.append(ways);

  // Empfangen
  const recv = card("Empfangen");
  const nameField = el("label", "field");
  nameField.append("Name");
  const name = el("input", "text-input") as HTMLInputElement;
  name.type = "text";
  name.maxLength = 40;
  name.value = s.share.name;
  name.placeholder = share?.alias && !s.share.name ? share.alias : "Notch · PC-Name";
  name.addEventListener("change", () => { s.share.name = name.value.trim(); commit(); });
  nameField.append(name);

  const folderField = el("div", "field");
  folderField.append("Ordner");
  const row = el("div", "input-row");
  const folder = el("input", "text-input") as HTMLInputElement;
  folder.type = "text";
  folder.value = s.share.folder;
  folder.placeholder = share?.folder ?? "Downloads\\Notch";
  folder.title = "Leer lassen für Downloads › Notch. Empfangenes liegt zusätzlich in der Ablage der Notch.";
  folder.addEventListener("change", () => { s.share.folder = folder.value.trim(); commit(); });
  const openF = el("button", "icon-btn n-glass n-liquid");
  openF.innerHTML = ICON_FOLDER;
  openF.title = "Ordner öffnen";
  openF.setAttribute("aria-label", "Ordner öffnen");
  openF.addEventListener("click", () => invoke("open", { target: s.share.folder || share?.folder || "" }).catch(() => {}));
  row.append(folder, openF);
  folderField.append(row);

  const trustField = el("div", "field");
  trustField.append("Ohne Nachfrage annehmen von");
  const tchips = el("div", "chips");
  if (!s.share.trusted.length) tchips.append(el("small", "faint", "Niemand – jede Sendung wird nachgefragt"));
  for (const t of s.share.trusted) {
    const b = el("button", "fav");
    b.setAttribute("aria-pressed", "true");
    b.title = `${t.alias} wieder nachfragen`;
    b.append(document.createTextNode(t.alias), el("span", "x", "×"));
    b.addEventListener("click", () => { s.share.trusted = s.share.trusted.filter((x) => x.fp !== t.fp); commit(); renderShare(); });
    tchips.append(b);
  }
  trustField.append(tchips);
  recv.append(nameField, folderField, trustField);
  rest.append(recv);

  // Anleitung (darf erklaeren)
  const how = el("details", "settings-group");
  how.open = !s.share.trusted.length && !near.length;
  how.append(el("summary", "", "So geht’s"));
  const body = el("div", "group-body howto");
  const steps = [
    "LocalSend aus dem App Store laden – kostenlos, ohne Konto. iPhone und PC im selben WLAN; fragt Windows nach dem Netzwerk, „Private Netzwerke“ erlauben.",
    `iPhone → PC: Teilen › LocalSend › „${share?.alias ?? "Notch"}“. Die Notch fragt: Annehmen, Ablehnen oder Immer annehmen. Text und Links zeigt sie zum Kopieren.`,
    "PC → iPhone: LocalSend auf dem iPhone offen lassen, in der Ablage der Notch Rechtsklick auf eine Datei › An iPhone senden.",
    "Ohne App: QR-Code scannen – Safari öffnet eine Seite zum Hoch- und Herunterladen.",
  ];
  const ol = el("ol", "steps");
  for (const t of steps) ol.append(el("li", "", t));
  body.append(ol, el("p", "fine tight", "Echtes AirDrop braucht Apples eigenes Funkprotokoll, das Windows nicht kann. Die Übertragung ist unverschlüsselt und fürs Heim-WLAN gedacht."));
  how.append(body);
  rest.append(how);

  parts.push(rest);
  box.replaceChildren(...parts);
  renderShareStatus();
}

function renderAll() {
  syncSegs();
  renderList();
  renderDock();
  renderSport();
  renderShare();
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
  // Musik-Reaktion: was gerade erkannt wird (hoechstens 4x pro Sekunde auffrischen)
  let reactShown = 0;
  await listen<{ s: string; bpm: number; beat: number; w?: number[] }>("spectrum", (e) => {
    reactLast = { ...e.payload, at: Date.now() };
    if (Date.now() - reactShown > 250) { reactShown = Date.now(); showReactHint(); }
  });
  setInterval(showReactHint, 2000);

  await listen<Media>("media", (e) => {
    if (liveMedia?.playing && e.payload && !e.payload.playing) pausedAt = Date.now();
    liveMedia = e.payload;
    refresh();
  });
  await listen<{ src: string }>("media-cover", (e) => { liveCover = e.payload.src; refresh(); });
  await listen<CAct[]>("activities", (e) => { liveActs = e.payload; refresh(); });
  const snap = await invoke<{ media: Media; cover: string | null; activities: CAct[]; dock: string; voice_key: string }>("snapshot").catch(() => null);
  if (snap) { liveMedia = snap.media; liveCover = snap.cover; liveActs = snap.activities; }

  // ohne laufende Quellen gleich zum Ausprobieren
  const c0 = current();
  if (!c0.plan.slots.length && !c0.plan.timer) mode = "demo";

  document.querySelectorAll<HTMLButtonElement>("[data-slots]").forEach((b) =>
    b.addEventListener("click", () => { s.slots = Number(b.dataset.slots) === 1 ? 1 : 2; syncSegs(); commit(); }));
  document.querySelectorAll<HTMLButtonElement>("[data-mode]").forEach((b) =>
    b.addEventListener("click", () => { mode = b.dataset.mode === "demo" ? "demo" : "live"; pvSig = ""; syncSegs(); refresh(); }));

  const bpm = q<HTMLInputElement>(".d-bpm"), bpmOut = q(".d-bpm-label");
  const bg = q<HTMLInputElement>(".d-bg"), bgOut = q(".d-bg-label");
  const syncDemo = () => {
    demoBpm = Number(bpm.value); demoBg = Number(bg.value);
    bpmOut.textContent = `Puls · ${demoBpm} bpm`;
    bgOut.textContent = `Blutzucker · ${demoBg} mg/dL`;
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

  // Reiter
  document.querySelectorAll<HTMLButtonElement>("[data-tab]").forEach((b) => b.addEventListener("click", () => setTab(b.dataset.tab!)));
  let startTab = "display";
  try { startTab = localStorage.getItem("settings-tab") || "display"; } catch { /* egal */ }
  // settings.html#app: aus der Notch geoeffnet, um den OpenAI-Schluessel einzutragen
  if (location.hash.length > 1) startTab = location.hash.slice(1);
  setTab(q(`[data-pane="${startTab}"]`) ? startTab : "display");
  (window as any).notchTab = (t: string) => { if (q(`[data-pane="${t}"]`)) setTab(t); if (t === "app") q<HTMLInputElement>(".key-input").focus(); };
  if (startTab === "app") q<HTMLInputElement>(".key-input").focus();

  // Live-Sport: Wettbewerbe aus sport.rs, Stand live mitlesen
  invoke<LeagueInfo[]>("sport_leagues").then((l) => { leagues = l; renderSport(); }).catch(() => {});
  invoke<boolean>("arena_installed").then((v) => { arenaThere = v; if (v) renderSport(); }).catch(() => {});
  sportLive = (snap as { sport?: SportLive } | null)?.sport ?? null;
  await listen<SportLive>("sport", (e) => { sportLive = e.payload; renderSportLive(); });

  // iPhone-Austausch: Zustand aus share.rs (laeuft, Adresse, Geraete in der Naehe)
  invoke<ShareState>("share_status").then((x) => { share = x; renderShare(); }).catch(() => {});
  await listen<ShareState>("share-state", (e) => {
    const was = JSON.stringify(share?.devices) + share?.running + share?.alias + share?.folder;
    share = e.payload;
    renderShareStatus();
    // Liste der Geraete neu, aber nicht beim Tippen in ein Feld
    const now = JSON.stringify(share.devices) + share.running + share.alias + share.folder;
    if (now !== was && !(document.activeElement as HTMLElement | null)?.closest?.(".share-options input")) renderShare();
  });

  // Andocken: gleiche Befehle wie Tray und Ziehen
  dock = snap?.dock ?? "top";
  await listen<string>("dock", (e) => { dock = e.payload; syncSegs(); });
  document.querySelectorAll<HTMLButtonElement>("button[data-dock]").forEach((b) =>
    b.addEventListener("click", () => { dock = b.dataset.dock!; syncSegs(); invoke("dock_set", { dock }).catch(() => {}); }));

  // App
  getVersion().then((v) => { q(".app-version").textContent = `Version ${v}`; }).catch(() => {});
  q(".voice-key").textContent = snap?.voice_key || "kein freies Kürzel gefunden";
  // OpenAI-Schluessel: wird nur hingeschickt (Rust prueft und verschluesselt), nie zurueckgelesen
  type KeyStatus = { source: "env" | "app" | "none"; hint: string | null };
  const keyInput = q<HTMLInputElement>(".key-input"), keyStatus = q(".key-status");
  const keySave = q<HTMLButtonElement>(".key-save"), keyRemove = q<HTMLButtonElement>(".key-remove");
  const showKey = (st: KeyStatus, msg?: string, error = false) => {
    keyStatus.textContent = msg ?? (st.source === "env" ? `Aus der Umgebungsvariable OPENAI_API_KEY (${st.hint}) – hat Vorrang.`
      : st.source === "app" ? `Gespeichert: ${st.hint}` : "Noch kein Schlüssel hinterlegt.");
    keyStatus.classList.toggle("error", error);
    keyRemove.hidden = st.source !== "app";
  };
  let keyState: KeyStatus = { source: "none", hint: null };
  invoke<KeyStatus>("voice_key_status").then((st) => { keyState = st; showKey(st); }).catch(() => {});
  const saveKey = async () => {
    if (!keyInput.value.trim()) { keyInput.focus(); return; }
    keySave.disabled = true;
    showKey(keyState, "Wird bei OpenAI geprüft …");
    try {
      keyState = await invoke<KeyStatus>("voice_key_save", { key: keyInput.value });
      keyInput.value = "";
      showKey(keyState, `Gespeichert: ${keyState.hint} – der Sprachassistent ist bereit.`);
      announce("OpenAI-Schlüssel gespeichert");
    } catch (e) {
      showKey(keyState, String(e), true);
    } finally {
      keySave.disabled = false;
    }
  };
  keySave.addEventListener("click", () => void saveKey());
  keyInput.addEventListener("keydown", (e) => { if (e.key === "Enter") void saveKey(); });
  keyRemove.addEventListener("click", async () => {
    keyState = await invoke<KeyStatus>("voice_key_remove");
    showKey(keyState, keyState.source === "env" ? undefined : "Schlüssel entfernt.");
    announce("OpenAI-Schlüssel entfernt");
  });

  // Fensterknoepfe als Glaspille (gemeinsame Designsprache), Esc schliesst
  const close = () => { getCurrentWindow().close().catch(() => {}); };
  windowControls({ minimize: () => void getCurrentWindow().minimize().catch(() => {}), close, closeTitle: "Schließen (Esc)" });
  glassLight();
  liquid();
  segments(document, ".n-seg, .segmented");
  // Lichtleiste statt Bildlaufleiste
  lightScroller(q(".panel-body"), { insetTop: 8 });
  document.addEventListener("keydown", (e) => { if (e.key === "Escape" && !(e.target as Element).closest?.("input[type=text]")) close(); });

  demoChips();
  renderAll();

  // Beispiel-Timer laeuft rueckwaerts; Musik-Nachlauf und veraltete Werte brauchen ab und zu einen Blick
  setInterval(() => {
    if (mode === "demo" && demoOn.has("timer")) demoTimerLeft = demoTimerLeft > 1 ? demoTimerLeft - 1 : 600;
    refresh();
  }, 1000);
}

main();
