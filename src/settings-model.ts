/**
 * Einstellungen der kompakten (zugeklappten) Notch: Rangliste + Extras pro Quelle.
 * Rust speichert das Objekt nur (config.json -> "compact") und verteilt Aenderungen per Event
 * "settings" an alle Fenster. Schema, Standardwerte und Pruefung leben hier.
 */

export type SourceId = "glucose" | "music" | "timer" | "pulse" | "folio" | "other";

export const SOURCE_IDS: SourceId[] = ["glucose", "music", "timer", "pulse", "folio", "other"];

export const SOURCES: Record<SourceId, { name: string; from: string; color: string }> = {
  glucose: { name: "Blutzucker", from: "Haze", color: "#ff7971" },
  music: { name: "Musik", from: "Windows-Medien", color: "#30d158" },
  timer: { name: "Timer", from: "Notch", color: "#ffb340" },
  pulse: { name: "Puls", from: "Garmin über Haze, sonst Helio", color: "#ff5a6e" },
  folio: { name: "Folio", from: "Seitenzahl", color: "#f5f5f7" },
  other: { name: "Andere Apps", from: "Live Activities", color: "#64d2ff" },
};

export type CompactSettings = {
  v: 1;
  /** oben = gewinnt den Platz in der kleinen Notch */
  order: SourceId[];
  /** nie in der kleinen Notch (aufgeklappt weiterhin sichtbar) */
  hidden: SourceId[];
  /** wie viele Dinge die kleine Notch gleichzeitig zeigt */
  slots: 1 | 2;
  glucose: { delta: boolean; outOfRangeTop: boolean };
  /** react: Kante der Notch bewegt sich zur Musik — auto = Anteile aus der Musik, manual = layers;
   *  strength 0.5..2 */
  music: { eqBesideCover: boolean; lingerSec: number; react: MusicReact; layers: Layers; strength: number };
  timer: { expand: boolean };
  pulse: { boost: boolean; threshold: number };
  folio: { flash: boolean; flashSec: number };
  /** Vollbild-Programm vorn: hide = weg; peek = weg, Maus an die Kante holt sie kurz raus;
   *  show = bleibt sichtbar, aber durchklickbar (Klicks gehen ans Programm) */
  fullscreen: { mode: FullscreenMode };
};

export type MusicReact = "off" | "auto" | "manual";
export const MUSIC_REACT: MusicReact[] = ["off", "auto", "manual"];
export type Layer = "spikes" | "wave" | "pulse";
export const LAYERS: Layer[] = ["spikes", "wave", "pulse"];
/** welche Stile bei „Eigene“ gleichzeitig laufen */
export type Layers = Record<Layer, boolean>;

export type FullscreenMode = "hide" | "peek" | "show";
export const FULLSCREEN_MODES: FullscreenMode[] = ["hide", "peek", "show"];

export const DEFAULTS: CompactSettings = {
  v: 1,
  order: ["glucose", "music", "timer", "pulse", "folio", "other"],
  hidden: [],
  slots: 2,
  glucose: { delta: true, outOfRangeTop: true },
  music: { eqBesideCover: true, lingerSec: 30, react: "off", layers: { spikes: true, wave: false, pulse: true }, strength: 1 },
  timer: { expand: true },
  pulse: { boost: true, threshold: 140 },
  folio: { flash: true, flashSec: 2 },
  fullscreen: { mode: "hide" },
};

/** Puls faellt erst so viele bpm unter der Schwelle wieder zurueck (kein Hin- und Herspringen) */
export const PULSE_HYST = 5;

const num = (v: unknown, lo: number, hi: number, d: number) => {
  const n = Number(v);
  return Number.isFinite(n) ? Math.min(hi, Math.max(lo, n)) : d;
};
const bool = (v: unknown, d: boolean) => (typeof v === "boolean" ? v : d);
const obj = (v: unknown): Record<string, unknown> => (v && typeof v === "object" ? (v as Record<string, unknown>) : {});

/** Beliebiges (altes, kaputtes, leeres) Objekt -> vollstaendige, gueltige Einstellungen */
export function normalize(raw: unknown): CompactSettings {
  const r = obj(raw);
  const known = (x: unknown): x is SourceId => SOURCE_IDS.includes(x as SourceId);
  const order = (Array.isArray(r.order) ? r.order : []).filter(known);
  const uniq = [...new Set(order)];
  // neue Quellen (spaetere Versionen) an ihrer Standardstelle einfuegen
  for (const id of DEFAULTS.order) if (!uniq.includes(id)) uniq.splice(Math.min(DEFAULTS.order.indexOf(id), uniq.length), 0, id);
  const g = obj(r.glucose), m = obj(r.music), t = obj(r.timer), p = obj(r.pulse), f = obj(r.folio), fs = obj(r.fullscreen);
  return {
    v: 1,
    order: uniq,
    hidden: [...new Set((Array.isArray(r.hidden) ? r.hidden : []).filter(known))],
    slots: r.slots === 1 ? 1 : 2,
    glucose: { delta: bool(g.delta, DEFAULTS.glucose.delta), outOfRangeTop: bool(g.outOfRangeTop, DEFAULTS.glucose.outOfRangeTop) },
    music: { eqBesideCover: bool(m.eqBesideCover, DEFAULTS.music.eqBesideCover), lingerSec: num(m.lingerSec, 0, 300, DEFAULTS.music.lingerSec),
      // 0.1.5 kannte nur einen Stil: "spikes" -> Eigene mit Zacken
      react: MUSIC_REACT.includes(m.react as MusicReact) ? (m.react as MusicReact) : LAYERS.includes(m.react as Layer) ? "manual" : DEFAULTS.music.react,
      layers: LAYERS.includes(m.react as Layer)
        ? { spikes: m.react === "spikes", wave: m.react === "wave", pulse: m.react === "pulse" }
        : layers(obj(m.layers)),
      strength: num(m.strength, 0.5, 2, DEFAULTS.music.strength) },
    timer: { expand: bool(t.expand, DEFAULTS.timer.expand) },
    pulse: { boost: bool(p.boost, DEFAULTS.pulse.boost), threshold: Math.round(num(p.threshold, 60, 220, DEFAULTS.pulse.threshold)) },
    folio: { flash: bool(f.flash, DEFAULTS.folio.flash), flashSec: num(f.flashSec, 0.5, 8, DEFAULTS.folio.flashSec) },
    fullscreen: { mode: FULLSCREEN_MODES.includes(fs.mode as FullscreenMode) ? (fs.mode as FullscreenMode) : DEFAULTS.fullscreen.mode },
  };
}

/** mindestens ein Stil bleibt an */
function layers(l: Record<string, unknown>): Layers {
  const out = { spikes: bool(l.spikes, DEFAULTS.music.layers.spikes), wave: bool(l.wave, DEFAULTS.music.layers.wave), pulse: bool(l.pulse, DEFAULTS.music.layers.pulse) };
  return out.spikes || out.wave || out.pulse ? out : { ...DEFAULTS.music.layers };
}

export const clone = (s: CompactSettings): CompactSettings => JSON.parse(JSON.stringify(s));
