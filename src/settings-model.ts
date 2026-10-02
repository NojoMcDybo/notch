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
  /** Live-Sport (sport.rs): Wettbewerbe, Lieblingsteams, wann die Notch aufklappt */
  sport: SportSettings;
  /** iPhone-Austausch (share.rs): LocalSend + Browser-Seite im WLAN */
  share: ShareSettings;
};

export type ShareSettings = {
  /** aus, bis man es einschaltet: dann lauscht die Notch im lokalen Netz (Port 53317) */
  on: boolean;
  /** Name, unter dem die Notch in LocalSend erscheint (leer = „Notch · PC-Name“) */
  name: string;
  /** Zielordner (leer = Downloads\Notch) */
  folder: string;
  /** Geraete, deren Sendungen ohne Nachfrage angenommen werden */
  trusted: { fp: string; alias: string }[];
};

export type SportExpand = "off" | "goals" | "important" | "all";
export const SPORT_EXPAND: SportExpand[] = ["off", "goals", "important", "all"];
export type SportTeam = { key: string; name: string; logo?: string };
export type SportSettings = {
  on: boolean;
  /** Wettbewerbe (ids aus sport.rs, z. B. "bl1", "dfbteam") */
  leagues: string[];
  teams: SportTeam[];
  /** all = alle Spiele der Wettbewerbe, fav = nur Spiele der Lieblingsteams */
  scope: "all" | "fav";
  /** bei welchen Meldungen die Notch kurz aufklappt (Tore / Wichtiges / alles / nie) */
  expand: SportExpand;
  /** Spielstand in der Mitte der kleinen Notch */
  center: boolean;
  /** Ballverlauf auf dem Spielfeld (laedt beim Ansehen jede Ballaktion nach) */
  pitch: boolean;
  /** im Vollbild bei einem Tor kurz den Spielstand zeigen (nicht bei „Ausblenden“) */
  fullscreen: boolean;
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
  sport: { on: true, leagues: ["bl1", "dfbteam"], teams: [], scope: "all", expand: "goals", center: true, pitch: true, fullscreen: true },
  share: { on: false, name: "", folder: "", trusted: [] },
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
  const g = obj(r.glucose), m = obj(r.music), t = obj(r.timer), p = obj(r.pulse), f = obj(r.folio), fs = obj(r.fullscreen), sp = obj(r.sport), sh = obj(r.share);
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
    sport: sport(sp),
    share: {
      on: bool(sh.on, DEFAULTS.share.on),
      name: typeof sh.name === "string" ? sh.name.slice(0, 40) : "",
      folder: typeof sh.folder === "string" ? sh.folder.slice(0, 260) : "",
      trusted: (Array.isArray(sh.trusted) ? sh.trusted : []).map(obj)
        .filter((t) => typeof t.fp === "string" && t.fp)
        .map((t) => ({ fp: String(t.fp).slice(0, 80), alias: typeof t.alias === "string" ? t.alias.slice(0, 40) : "Gerät" }))
        .slice(0, 20),
    },
  };
}

function sport(sp: Record<string, unknown>): SportSettings {
  const d = DEFAULTS.sport;
  const id = (x: unknown) => typeof x === "string" && /^[a-z0-9]{2,12}$/.test(x);
  const teams = (Array.isArray(sp.teams) ? sp.teams : [])
    .map(obj)
    .filter((t) => typeof t.key === "string" && t.key && typeof t.name === "string")
    .map((t) => ({ key: String(t.key).slice(0, 40), name: String(t.name).slice(0, 60), ...(typeof t.logo === "string" ? { logo: t.logo.slice(0, 300) } : {}) }));
  return {
    on: bool(sp.on, d.on),
    leagues: Array.isArray(sp.leagues) ? [...new Set(sp.leagues.filter(id) as string[])] : [...d.leagues],
    teams: teams.filter((t, i) => teams.findIndex((x) => x.key === t.key) === i).slice(0, 20),
    scope: sp.scope === "fav" ? "fav" : "all",
    expand: SPORT_EXPAND.includes(sp.expand as SportExpand) ? (sp.expand as SportExpand) : d.expand,
    center: bool(sp.center, d.center),
    pitch: bool(sp.pitch, d.pitch),
    fullscreen: bool(sp.fullscreen, d.fullscreen),
  };
}

/** mindestens ein Stil bleibt an */
function layers(l: Record<string, unknown>): Layers {
  const out = { spikes: bool(l.spikes, DEFAULTS.music.layers.spikes), wave: bool(l.wave, DEFAULTS.music.layers.wave), pulse: bool(l.pulse, DEFAULTS.music.layers.pulse) };
  return out.spikes || out.wave || out.pulse ? out : { ...DEFAULTS.music.layers };
}

export const clone = (s: CompactSettings): CompactSettings => JSON.parse(JSON.stringify(s));
