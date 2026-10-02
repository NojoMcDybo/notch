/**
 * Kompakte (zugeklappte) Notch: Was bekommt den Platz?
 *
 * 1. Jede Activity gehoert zu einer Quelle (Blutzucker, Musik, Timer, Puls, Folio, Andere).
 * 2. Die Rangliste aus den Einstellungen entscheidet; zwei Regeln schieben live nach oben:
 *    Puls ab einer Schwelle (mit Hysterese), Blutzucker ausserhalb des Zielbereichs (gewinnt).
 * 3. Die ersten `slots` aktiven, nicht ausgeblendeten Quellen werden gezeigt.
 * 4. Laeuft ein Timer und "verbreitern" ist an, nimmt er keinen Platz weg: die Notch wird breiter.
 * 5. Blaettert man in Folio, waehrend etwas anderes oben liegt, blendet die Seitenzahl kurz ein.
 *
 * Wird von der Notch (main.ts) und von der Vorschau im Einstellungsfenster benutzt.
 */

import { bgOutOfRange, bgSig, fillCompact, isBg, type ChartData } from "./glucose";
import { PULSE_HYST, type CompactSettings, type SourceId } from "./settings-model";

export type CAct = {
  id: string; app: string; title: string; value?: string; unit?: string; icon?: string; color?: string;
  priority: number; updated: number; pulse?: number; trend?: string; delta?: number; chart?: ChartData; expired?: boolean;
};

export const TIMER_ID = "notch:timer";

export function sourceOf(a: CAct): SourceId {
  if (a.id === TIMER_ID) return "timer";
  if (isBg(a) || a.app === "Haze") return "glucose";
  if ((a.pulse ?? 0) > 0 || a.app === "Helio") return "pulse";
  if (a.app === "Folio") return "folio";
  return "other";
}

export const bpmOf = (a: CAct) => (a.pulse && a.pulse > 0 ? a.pulse : Number(a.value) || 0);

/** Puls-Schwelle mit Hysterese: rueckt ab `threshold` hoch, faellt erst unter threshold − 5 zurueck. */
export class PulseGate {
  on = false;
  update(bpm: number, s: CompactSettings) {
    if (!s.pulse.boost || !bpm) return (this.on = false);
    this.on = this.on ? bpm >= s.pulse.threshold - PULSE_HYST : bpm >= s.pulse.threshold;
    return this.on;
  }
}

export type Slot = { src: SourceId; act?: CAct; flash?: boolean };
export type Plan = {
  slots: Slot[];
  /** laufender Timer als Erweiterung (nimmt keinen Platz in der Rangliste) */
  timer?: CAct;
  /** effektive Reihenfolge nach Live-Regeln (fuer die Anzeige im Einstellungsfenster) */
  order: SourceId[];
  boosted: { pulse: boolean; glucose: boolean };
  /** Quellen, die gerade etwas zu zeigen haetten */
  active: Set<SourceId>;
};

export type PlanInput = {
  acts: CAct[];
  /** Musik laeuft (oder Nachlauf nach Pause) */
  music: boolean;
  s: CompactSettings;
  gate: PulseGate;
  /** id der Folio-Activity, deren Seite sich gerade geaendert hat (oder null) */
  folioFlash?: string | null;
};

export function plan({ acts, music, s, gate, folioFlash }: PlanInput): Plan {
  // pro Quelle der wichtigste Eintrag (Rust liefert schon nach priority, dann Aktualitaet sortiert)
  const top = new Map<SourceId, CAct>();
  for (const a of acts) {
    const src = sourceOf(a);
    if (!top.has(src)) top.set(src, a);
  }
  const active = new Set<SourceId>(top.keys());
  if (music) active.add("music");

  const pulse = top.get("pulse");
  const pulseUp = gate.update(pulse ? bpmOf(pulse) : 0, s);
  const bg = top.get("glucose");
  const bgUp = !!bg && s.glucose.outOfRangeTop && isBg(bg) && bgOutOfRange(bg);

  let order = [...s.order];
  const toFront = (id: SourceId) => { order = [id, ...order.filter((x) => x !== id)]; };
  if (pulseUp) toFront("pulse");
  if (bgUp) toFront("glucose"); // Blutzucker ausserhalb des Bereichs schlaegt hohen Puls

  const hidden = new Set(s.hidden);
  let timer: CAct | undefined;
  if (s.timer.expand && top.has("timer") && !hidden.has("timer")) timer = top.get("timer");

  const ranked = order.filter((id) => active.has(id) && !hidden.has(id) && !(timer && id === "timer"));
  const slots: Slot[] = ranked.slice(0, s.slots).map((src) => ({ src, act: top.get(src) }));

  // Folio blaettert, liegt aber nicht in der Notch -> kurz den letzten Platz uebernehmen
  const folio = top.get("folio");
  if (folioFlash && folio && folio.id === folioFlash && s.folio.flash && !hidden.has("folio") && !slots.some((x) => x.src === "folio")) {
    const f: Slot = { src: "folio", act: folio, flash: true };
    if (slots.length < s.slots) slots.push(f);
    else slots[slots.length - 1] = f;
  }

  return { slots, timer, order, boosted: { pulse: pulseUp, glucose: bgUp }, active };
}

/** Signatur: nur neu bauen, wenn sich etwas Sichtbares aendert (sonst startet der Pegel dauernd neu) */
export function planSig(p: Plan, extra: unknown[] = []) {
  const a = (x?: CAct) => (x ? [x.id, x.value, x.unit, x.color, x.icon?.length, isBg(x) ? bgSig(x) : ""].join("~") : "");
  return JSON.stringify([p.slots.map((x) => [x.src, x.flash, a(x.act)]), a(p.timer), ...extra]);
}
/** Welche Quellen stehen auf den Plaetzen — wechselt das, blenden die Elemente weich ein */
export const planKey = (p: Plan) => p.slots.map((x) => x.src + (x.act?.id ?? "")).join("|") + (p.timer ? "|T" : "");

export type BuildCtx = {
  coverSrc: string | null;
  musicPlaying: boolean;
  iconEl: (a: CAct) => HTMLElement;
  eq: (playing: boolean) => HTMLElement;
};

function mk<K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
}

function coverEl(ctx: BuildCtx) {
  if (ctx.coverSrc) {
    const img = new Image();
    img.className = "c-cover";
    img.src = ctx.coverSrc;
    return img;
  }
  const n = mk("div", "icon c-note", "♪");
  return n;
}

/** Wert einer Activity (ohne Symbol). Blutzucker: Wert + Pfeil + Aenderung. */
function valueEl(a: CAct, s: CompactSettings) {
  const v = mk("div", "chip-v");
  if (isBg(a)) {
    fillCompact(v, a, { delta: s.glucose.delta });
    return v;
  }
  if (a.color) v.style.setProperty("--accent", a.color);
  v.append(mk("span", "", a.value ?? ""));
  if (a.unit && a.value) v.append(mk("small", "", a.unit));
  return v;
}

/** Symbol + Wert als ein Stueck (wenn zwei Dinge nebeneinander stehen) */
function chipEl(a: CAct, s: CompactSettings, ctx: BuildCtx, src: SourceId) {
  const c = mk("div", "chip");
  c.dataset.src = src;
  if (a.color) c.style.setProperty("--accent", a.color);
  // Blutzucker ist am Pfeil erkennbar, braucht kein Symbol
  if (!isBg(a)) c.append(ctx.iconEl(a));
  c.append(valueEl(a, s));
  return c;
}

function timerEl(a: CAct, ctx: BuildCtx) {
  const t = mk("div", "chip c-timer");
  t.dataset.src = "timer";
  t.style.setProperty("--accent", a.color ?? "#ffb340");
  t.append(ctx.iconEl(a), mk("span", "chip-v", a.value ?? ""));
  return t;
}

/**
 * Baut links (lead) und rechts (trail). Regeln:
 * - eine Quelle: Symbol/Cover links, Wert/Pegel rechts (wie bisher)
 * - zwei Quellen: Musik immer links (Cover, Pegel daneben), sonst Rang 1 links, Rang 2 rechts
 * - Timer-Erweiterung: rechts angehaengt, die Notch wird so breit wie noetig
 */
export function build(lead: HTMLElement, trail: HTMLElement, p: Plan, s: CompactSettings, ctx: BuildCtx, animate = false) {
  lead.replaceChildren();
  trail.replaceChildren();
  trail.className = "c-trail";
  trail.style.removeProperty("--accent");

  const music = p.slots.find((x) => x.src === "music");
  const rest = p.slots.filter((x) => x.src !== "music");
  const L: HTMLElement[] = [];
  const R: HTMLElement[] = [];

  if (music) {
    L.push(coverEl(ctx));
    const rightBusy = rest.length > 0 || !!p.timer;
    if (!rightBusy) R.push(ctx.eq(ctx.musicPlaying));
    else if (s.music.eqBesideCover) L.push(ctx.eq(ctx.musicPlaying));
    if (rest[0]?.act) R.push(chipEl(rest[0].act, s, ctx, rest[0].src));
  } else if (rest.length === 1 && rest[0].act) {
    // eine Quelle allein: klassisch aufgeteilt
    const a = rest[0].act;
    L.push(ctx.iconEl(a));
    const v = valueEl(a, s);
    v.dataset.src = rest[0].src;
    R.push(v);
  } else if (rest.length >= 2) {
    if (rest[0].act) L.push(chipEl(rest[0].act, s, ctx, rest[0].src));
    if (rest[1].act) R.push(chipEl(rest[1].act, s, ctx, rest[1].src));
  }

  if (p.timer) {
    if (!L.length && !R.length) {
      // nur der Timer: Symbol links, Zeit rechts
      L.push(ctx.iconEl(p.timer));
      const v = mk("div", "chip-v");
      v.style.setProperty("--accent", p.timer.color ?? "#ffb340");
      v.append(mk("span", "", p.timer.value ?? ""));
      R.push(v);
    } else {
      if (R.length) R.push(mk("i", "c-sep"));
      R.push(timerEl(p.timer, ctx));
    }
  }

  for (const e of [...L, ...R]) {
    if (animate) e.classList.add("in");
    const flash = e.dataset.src === "folio" && p.slots.some((x) => x.flash);
    if (flash) e.classList.add("flash");
  }
  lead.append(...L);
  trail.append(...R);
}

/** Breite (oben) bzw. Hoehe (seitlich), die der Inhalt braucht */
export function needed(lead: HTMLElement, trail: HTMLElement, side: boolean, base: number) {
  const GAP = 56; // Luft in der Mitte, damit links und rechts als zwei Dinge lesbar bleiben
  const PAD = 22;
  const n = side ? lead.offsetHeight + trail.offsetHeight + PAD + GAP / 2 : lead.offsetWidth + trail.offsetWidth + PAD + GAP;
  // laufender Timer neben anderem Inhalt: die Notch wird sichtbar um sein Stueck groesser
  const t = trail.querySelector<HTMLElement>(".c-timer");
  const extra = t ? (side ? t.offsetHeight : t.offsetWidth) + 14 : 0;
  return Math.max(base + extra, Math.ceil(n));
}
