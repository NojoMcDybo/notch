/**
 * Live-Sport in der Notch (Daten: src-tauri/src/sport.rs).
 *
 * - Ueberall Wappen statt Vereinsnamen (Name im Tooltip); ohne Wappen das Kuerzel in der Teamfarbe.
 * - Mitte der kleinen Notch: Wappen 2:1 Wappen und Minute; nach einem Tor leuchtet der Stand in der Teamfarbe.
 * - Neue Meldung: die Notch klappt kurz auf, die Meldung schwebt als Liquid Glass ueber einem Licht in der
 *   Teamfarbe (die Glaskante bricht es).
 * - Aufgeklappt: Karte mit Spielstand, Spielfeld mit Ballverlauf (Beschriftung als Glas darueber), Ticker und
 *   weitere laufende Spiele.
 *
 * Ballverlauf: echte Positionsdaten aller Spieler gibt es live nicht frei. Die Notch spielt stattdessen jede
 * Ballaktion (Pass von wo nach wo, Flanke, Schuss, Zweikampf …) im Takt der echten Uhrzeit nach: der Ball
 * wandert von Aktion zu Aktion, die beteiligten Spieler erscheinen mit Rueckennummer in ihrer Teamfarbe.
 */

import { icon } from "./nojo/nojo-ui";
import { morph } from "./morph";

export type SportTeam = { id: string; name: string; short: string; abbr: string; logo: string; color: string; score: string };
export type SportEv = { id: string; minute: string; kind: string; side: string; title: string; text: string; big: number };
export type SportMatch = {
  key: string; league: string; league_name: string; sport: string; home: SportTeam; away: SportTeam;
  state: "pre" | "in" | "post"; clock: string; start: number; fav: boolean; link: string; pitch: boolean;
  /** Basketball (ESPN): Wurfbild verfuegbar */
  court?: boolean;
  events: SportEv[]; source: string;
};
export type SportState = { matches: SportMatch[]; error?: string; updated?: number; off?: boolean };
export type SportNews = { key: string; ev: SportEv; score: string; at: number };
export type SportPlay = {
  id: string; t: number; minute: string; kind: string; side: string; jersey: string; who: string;
  x: number; y: number; x2?: number | null; y2?: number | null;
  /** Basketball: Treffer, Punkte des Versuchs (kind "2" | "3" | "ft") */
  made?: boolean; pts?: number;
};

function mk<K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
}
const SVGNS = "http://www.w3.org/2000/svg";
function sv<K extends keyof SVGElementTagNameMap>(tag: K, attrs: Record<string, string | number> = {}) {
  const e = document.createElementNS(SVGNS, tag);
  for (const k in attrs) e.setAttribute(k, String(attrs[k]));
  return e;
}

/** Symbole aus der Bibliothek (src/nojo) statt Emoji oder eigener Zeichnungen */
const BALL_SVG = icon("ball");
const OPEN_SVG = icon("open");
/** Arena: Spielfeld in fein */
const ARENA_SVG = icon("arena");
const SPORT_GLYPH: Record<string, string> = { hockey: "●", football: "◆", basketball: "●", baseball: "●" };

/** Anstoss: "20:30" heute, sonst "Sa 15:30" */
export function kickoff(m: SportMatch) {
  const d = new Date(m.start);
  const t = d.toLocaleTimeString("de-DE", { hour: "2-digit", minute: "2-digit" });
  const today = new Date().toDateString() === d.toDateString();
  return today ? t : `${d.toLocaleDateString("de-DE", { weekday: "short" }).replace(".", "")} ${t}`;
}

/** ESPN liefert 500-px-Wappen; fuer die Notch reicht eine kleine Fassung (faellt sonst aufs Original zurueck) */
function smallLogo(url: string) {
  const m = /^https:\/\/a\.espncdn\.com(\/i\/teamlogos\/[^?]+\.png)$/.exec(url);
  return m ? `https://a.espncdn.com/combiner/i?img=${m[1]}&h=96&w=96` : url;
}

/** Wappen-Adressen, die nicht laden (bzw. deren kleine Fassung nicht): beim Neuzeichnen gleich richtig */
const logoBroken = new Set<string>();

/** Wappen statt Name; ohne Bild das Kuerzel mit einem Strich in der Teamfarbe */
export function crestEl(t: SportTeam, cls = "crest") {
  const c = mk("span", cls);
  c.style.setProperty("--c", t.color);
  c.title = t.name;
  const abbr = (t.abbr || t.short || t.name).slice(0, 3).toUpperCase();
  const small = t.logo ? smallLogo(t.logo) : "";
  const src = !t.logo || logoBroken.has(t.logo) ? "" : logoBroken.has(small) ? t.logo : small;
  if (!src) {
    c.classList.add("mono");
    c.textContent = abbr;
    return c;
  }
  const img = new Image();
  img.alt = "";
  img.draggable = false;
  img.src = src;
  // das Wappen kann inzwischen in einem anderen Element stecken (morph behaelt das alte): an den echten Eltern
  img.onerror = () => {
    logoBroken.add(img.src === small ? small : t.logo);
    if (img.src === small && small !== t.logo) { img.src = t.logo; return; }
    const host = img.parentElement;
    img.remove();
    if (host) { host.classList.add("mono"); host.textContent = abbr; }
  };
  c.append(img);
  return c;
}

/** Vereinsnamen aus Meldungen nehmen (das Wappen steht daneben): „Tor für Mainz!“ -> „Tor!“ */
export function deTeam(text: string, m?: SportMatch) {
  if (!m || !text) return text;
  let r = text;
  for (const n of [m.home.name, m.away.name, m.home.short, m.away.short]) if (n && n.length > 1) r = r.split(n).join("");
  r = r.replace(/\s*für\s*!/, "!").replace(/\(\s*\)/g, "").replace(/\s{2,}/g, " ").replace(/\s+([!.,:])/g, "$1").trim();
  return /^[\s–-]*$/.test(r) ? "" : r;
}

const sideTeam = (m: SportMatch, side: string) => (side === "home" ? m.home : side === "away" ? m.away : null);

/** Wappen 2:1 Wappen — der Stand, wie er ueberall in der Notch steht */
function bugEl(m: SportMatch, score: string, cls = "bug", lit = "") {
  const b = mk("span", cls);
  const h = crestEl(m.home), a = crestEl(m.away);
  if (lit === "home") h.classList.add("lit");
  if (lit === "away") a.classList.add("lit");
  b.append(h, mk("b", "", score), a);
  return b;
}

const scoreOf = (m: SportMatch) => (m.state === "pre" ? "–" : `${m.home.score || 0}:${m.away.score || 0}`);
/** Anstoss vorbei, ESPN meldet aber noch nichts (manche Testspiele ueberträgt ESPN gar nicht live; wie in Arena) */
export const noLive = (m: SportMatch) => m.state === "pre" && m.source === "espn" && Date.now() - m.start > 15 * 60_000;
const NO_LIVE = "ESPN überträgt dieses Spiel nicht live";

export const clockOf = (m: SportMatch) => (m.state === "pre" ? kickoff(m) : m.clock || (m.state === "post" ? "Ende" : ""));

/** Farbe der Mannschaft, die die Meldung betrifft (Tor: der Torschuetze) */
export function newsColor(n: SportNews, m?: SportMatch) {
  if (n.ev.kind === "red") return "#ff453a";
  if (n.ev.kind === "yellow") return "#ffd60a";
  if (!m) return "#ffffff";
  return n.ev.side === "away" ? m.away.color : n.ev.side === "home" ? m.home.color : "#ffffff";
}

// ---------- Mitte der kleinen Notch ----------

export function midSig(m: SportMatch, hot: SportNews | null) {
  return [m.key, m.home.score, m.away.score, clockOf(m), m.state, hot?.ev.id ?? ""].join("|");
}

/** clock: Minute in der Mitte zeigen (aus, wenn die Spielzeit als eigenes Element am Rand steht) */
export function midEl(m: SportMatch, hot: SportNews | null, clock = true) {
  const box = mk("div", `sm ${m.state}` + (hot ? " hot" : ""));
  box.style.setProperty("--hc", m.home.color);
  box.style.setProperty("--ac", m.away.color);
  if (hot) box.style.setProperty("--hot", newsColor(hot, m));
  box.title = `${m.home.name} – ${m.away.name} · ${m.league_name}`;
  const h = crestEl(m.home, "sm-t h"), a = crestEl(m.away, "sm-t a");
  if (hot?.ev.side === "home") h.classList.add("lit");
  if (hot?.ev.side === "away") a.classList.add("lit");
  box.dataset.key = m.key;
  box.append(h, mk("span", "sm-s", scoreOf(m)), a);
  if (clock) box.append(mk("small", "sm-c", clockOf(m)));
  return box;
}

// ---------- Meldung oben ----------

const KIND_ICON: Record<string, string> = {
  goal: "", score: "", red: "", yellow: "", sub: "⇄", miss: "✕", var: "VAR", wood: "◎", chance: "◎",
  corner: "⚑", offside: "⚐", kickoff: "▶", half: "Ⅱ", end: "■", info: "•",
};

function kindIcon(kind: string, sport: string) {
  const i = mk("span", `tk-i k-${kind}`);
  if ((kind === "goal" || kind === "score") && sport === "soccer") i.innerHTML = BALL_SVG;
  else i.textContent = kind === "goal" || kind === "score" ? SPORT_GLYPH[sport] ?? "●" : KIND_ICON[kind] ?? "•";
  return i;
}

export function bannerEl(n: SportNews, m: SportMatch | undefined) {
  const wrap = mk("div", "sp-news-wrap");
  wrap.style.setProperty("--c", newsColor(n, m));
  // Licht hinter dem Glas: die Kante bricht es, die Mitte zeigt es weich
  wrap.append(mk("i", "spn-glow"));
  const b = mk("div", `sp-news n-liquid panel k-${n.ev.kind}` + (n.ev.big >= 3 ? " big" : ""));
  b.dataset.refract = "9";
  b.append(kindIcon(n.ev.kind, m?.sport ?? "soccer"));
  const body = mk("div", "spn-body");
  body.append(mk("b", "", deTeam(n.ev.title, m) || n.ev.title));
  const sub = [deTeam(n.ev.text, m), n.ev.minute].filter(Boolean).join(" · ");
  if (sub) body.append(mk("span", "", sub));
  b.append(body);
  if (m) b.append(bugEl(m, n.score || scoreOf(m), "spn-score", n.ev.side));
  if (n.ev.big >= 3) b.append(mk("i", "spn-shine"));
  wrap.append(b);
  return wrap;
}

// ---------- Karte (aufgeklappt) ----------

export type CardOpts = {
  pitch: Pitch | null;
  /** Basketball: Wurfbild statt Ticker */
  court?: Court | null;
  onOpen: (m: SportMatch) => void;
  /** Arena (eigene Sport-App) ist installiert: Spiel dort gross oeffnen */
  onArena?: (m: SportMatch) => void;
  onFocus: (key: string) => void;
  side: boolean;
};

/** Ticker: neueste Meldung oben, hoechstens n Zeilen; Wappen statt Vereinsname */
function tickerEl(m: SportMatch, n: number) {
  const ol = mk("ol", "sp-ticker");
  const evs = [...m.events].reverse().slice(0, n);
  if (!evs.length) {
    ol.append(mk("li", "sp-empty", noLive(m) ? NO_LIVE : m.state === "pre" ? `Anstoß ${kickoff(m)}` : m.source === "openligadb" ? "Tore erscheinen hier" : "Noch keine Meldungen"));
    return ol;
  }
  for (const e of evs) {
    const li = mk("li", `k-${e.kind}` + (e.big >= 3 ? " big" : ""));
    li.dataset.key = e.id;
    li.style.setProperty("--c", e.side === "away" ? m.away.color : e.side === "home" ? m.home.color : "var(--fg2)");
    li.append(mk("span", "tk-m", e.minute), kindIcon(e.kind, m.sport));
    const t = mk("span", "tk-t");
    const team = sideTeam(m, e.side);
    if (team) t.append(crestEl(team, "crest tk-c"));
    t.append(mk("b", "", deTeam(e.title, m) || e.title));
    const text = deTeam(e.text, m);
    if (text) t.append(" ", mk("span", "", text));
    li.append(t);
    ol.append(li);
  }
  return ol;
}

/** Signatur der Karte: nur neu bauen, wenn sich Sichtbares aendert (der Ballverlauf lebt weiter) */
export function cardSig(focus: SportMatch | undefined, others: SportMatch[], side: boolean, pitchOn: boolean, arena = false) {
  const f = focus ? [focus.key, focus.home.score, focus.away.score, focus.clock, focus.state, noLive(focus), focus.events.map((e) => e.id).join(",")] : [];
  return JSON.stringify([f, others.map((m) => [m.key, m.home.score, m.away.score, m.clock]), side, pitchOn, arena]);
}

export function cardEl(focus: SportMatch, others: SportMatch[], o: CardOpts) {
  const wrap = mk("section", "sport");
  wrap.dataset.key = focus.key;
  const card = mk("div", `sp-card ${focus.state}`);
  card.style.setProperty("--hc", focus.home.color);
  card.style.setProperty("--ac", focus.away.color);

  const head = mk("div", "sp-head");
  head.append(mk("span", "sp-league", focus.league_name));
  head.append(mk("span", `sp-clock ${focus.state}`, noLive(focus) ? `${kickoff(focus)} · keine Live-Daten` : clockOf(focus)));
  const open = mk("button", "sp-open n-liquid");
  open.innerHTML = OPEN_SVG;
  open.title = "Spiel im Browser öffnen";
  open.setAttribute("aria-label", "Spiel im Browser öffnen");
  open.hidden = !focus.link;
  open.addEventListener("click", (e) => { e.stopPropagation(); o.onOpen(focus); });
  if (o.onArena) {
    const arena = mk("button", "sp-open n-liquid");
    arena.innerHTML = ARENA_SVG;
    arena.title = "In Arena öffnen – Spielfeld, Spielplan, Tabelle";
    arena.setAttribute("aria-label", "In Arena öffnen");
    arena.addEventListener("click", (e) => { e.stopPropagation(); o.onArena!(focus); });
    head.append(arena);
  }
  head.append(open);

  // Wappen  1 : 2  Wappen — Namen stehen im Tooltip
  const score = mk("div", "sp-score");
  score.title = `${focus.home.name} – ${focus.away.name}`;
  const num = mk("div", "sp-num");
  if (focus.state === "pre") num.append(mk("span", "sp-ko", kickoff(focus)));
  else num.append(mk("b", "", focus.home.score || "0"), mk("i", "", ":"), mk("b", "", focus.away.score || "0"));
  score.append(crestEl(focus.home, "crest sp-crest h"), num, crestEl(focus.away, "crest sp-crest a"));
  card.append(head, score);

  const body = mk("div", "sp-body" + (o.pitch ? " with-pitch" : "") + (o.court ? " with-court" : ""));
  if (o.court) body.append(o.court.el);
  else {
    if (o.pitch) body.append(o.pitch.el);
    body.append(tickerEl(focus, o.pitch ? (o.side ? 3 : 5) : 4));
  }
  card.append(body);
  wrap.append(card);

  if (others.length) {
    const more = mk("div", "sp-more");
    for (const m of others.slice(0, 6)) {
      const c = mk("button", `sp-chip n-liquid ${m.state}`);
      c.dataset.key = m.key;
      c.title = `${m.home.name} – ${m.away.name} · ${m.league_name}`;
      c.append(bugEl(m, scoreOf(m), "sp-bug"), mk("small", "", clockOf(m)));
      c.addEventListener("click", (e) => { e.stopPropagation(); o.onFocus(m.key); });
      more.append(c);
    }
    wrap.append(more);
  }
  return wrap;
}

// ---------- Spielfeld mit Ballverlauf ----------

const PLAY_DE: Record<string, string> = {
  pass: "Pass", cross: "Flanke", "ball-touch": "Ballkontakt", clear: "Klärung", out: "Aus", aerial: "Kopfballduell",
  tackle: "Zweikampf", "attempted-tackle": "Zweikampf", "throw-in": "Einwurf", "free-kick": "Freistoß", foul: "Foul",
  "take-on": "Dribbling", interception: "Ballgewinn", "goal-kick": "Abstoß", dispossessed: "Ballverlust", save: "Parade",
  "blocked-pass": "Pass geblockt", "shot-off-target": "Schuss vorbei", "shot-on-target": "Schuss aufs Tor",
  "shot-blocked": "Schuss geblockt", "corner-awarded": "Ecke", offside: "Abseits", goal: "TOR", "goal---header": "TOR (Kopfball)",
  "penalty---scored": "TOR (Elfmeter)", "keeper-sweeper": "Torwart kommt raus", "shield-ball-opp": "Ball abgeschirmt",
  "drop-of-ball": "Schiedsrichterball", handball: "Handspiel", substitution: "Wechsel", "corner-kick": "Ecke",
  "keeper-pick-up": "Torwart nimmt auf", claim: "Torwart fängt", punch: "Torwart faustet", smother: "Torwart klärt",
};
const playName = (k: string) => PLAY_DE[k] ?? (k.startsWith("penalty") ? "Elfmeter" : k.startsWith("goal") ? "TOR" : k.replace(/-/g, " "));
const isShot = (k: string) => k.startsWith("shot") || k.startsWith("goal") || k.startsWith("penalty");
const isGoal = (k: string) => k.startsWith("goal") || k === "penalty---scored";

const W = 105, H = 68;
const px = (x: number) => Math.max(-1.5, Math.min(W + 1.5, (x / 100) * W));
const py = (y: number) => Math.max(-1.5, Math.min(H + 1.5, (y / 100) * H));
const lum = (c: string) => {
  const m = /^#?([0-9a-f]{6})$/i.exec(c);
  if (!m) return 255;
  const n = parseInt(m[1], 16);
  return 0.2126 * (n >> 16) + 0.7152 * ((n >> 8) & 255) + 0.0722 * (n & 255);
};

type Step = { play: SportPlay; dur: number };
type Actor = { g: SVGGElement; at: number };

export class Pitch {
  readonly el: HTMLElement;
  private svg: SVGSVGElement;
  private trailG: SVGGElement;
  private shotG: SVGGElement;
  private actorG: SVGGElement;
  private ball: SVGGElement;
  private label: HTMLElement;
  private net: { l: SVGRectElement; r: SVGRectElement };
  private tagH: SVGGElement;
  private tagA: SVGGElement;
  private teams: { home: SportTeam; away: SportTeam } | null = null;
  private key = "";
  private colors = { home: "#4da3ff", away: "#ff6b6b" };
  private queue: Step[] = [];
  private pos = { x: W / 2, y: H / 2 };
  private tween: { fx: number; fy: number; tx: number; ty: number; t0: number; dur: number } | null = null;
  private stepEnd = 0;
  private raf = 0;
  private running = false;
  private actors = new Map<string, Actor>();
  private trail: { x: number; y: number }[] = [];
  private lastT = 0;

  constructor() {
    this.el = mk("div", "pitch");
    // lebt selbst weiter: morph() setzt es nur ein, baut es nie nach
    this.el.dataset.key = "pitch";
    this.el.dataset.keep = "";
    this.svg = sv("svg", { viewBox: `-3 -3 ${W + 6} ${H + 6}`, class: "pitch-svg", "aria-hidden": "true" });
    const lines = sv("g", { class: "pl" });
    lines.append(
      sv("rect", { x: 0, y: 0, width: W, height: H, rx: 1 }),
      sv("line", { x1: W / 2, y1: 0, x2: W / 2, y2: H }),
      sv("circle", { cx: W / 2, cy: H / 2, r: 9.15 }),
      sv("rect", { x: 0, y: 13.85, width: 16.5, height: 40.3 }),
      sv("rect", { x: W - 16.5, y: 13.85, width: 16.5, height: 40.3 }),
      sv("rect", { x: 0, y: 24.85, width: 5.5, height: 18.3 }),
      sv("rect", { x: W - 5.5, y: 24.85, width: 5.5, height: 18.3 }),
      sv("circle", { cx: W / 2, cy: H / 2, r: 0.5, class: "pl-dot" }),
    );
    // Wappen der Teams blass im Hintergrund ihrer Haelfte (Heim links, spielt nach rechts)
    this.tagH = sv("g", { class: "pitch-tag h" });
    this.tagA = sv("g", { class: "pitch-tag a" });
    lines.append(this.tagH, this.tagA);
    const l = sv("rect", { x: -2, y: 30.34, width: 2, height: 7.32, class: "net" });
    const r = sv("rect", { x: W, y: 30.34, width: 2, height: 7.32, class: "net" });
    this.net = { l, r };
    this.trailG = sv("g", { class: "trail" });
    this.shotG = sv("g", { class: "shots" });
    this.actorG = sv("g", { class: "actors" });
    this.ball = sv("g", { class: "ball" });
    this.ball.append(sv("circle", { r: 1.25 }));
    this.svg.append(lines, l, r, this.trailG, this.shotG, this.actorG, this.ball);
    // Beschriftung schwebt als Liquid Glass ueber dem Feld: Ball und Spieler laufen darunter durch
    this.label = mk("div", "pitch-label n-liquid", "Ballverlauf wird geladen …");
    this.el.append(this.svg, this.label);
    this.place();
  }

  /** Spiel wechseln: Farben uebernehmen, bei anderem Spiel alles leeren */
  setMatch(m: SportMatch) {
    this.colors = { home: m.home.color, away: m.away.color };
    this.el.style.setProperty("--hc", m.home.color);
    this.el.style.setProperty("--ac", m.away.color);
    this.teams = { home: m.home, away: m.away };
    this.tag(this.tagH, m.home, W / 4);
    this.tag(this.tagA, m.away, (W * 3) / 4);
    if (m.key !== this.key) {
      this.key = m.key;
      this.clear();
      this.label.textContent = m.state === "pre" ? "Ballverlauf ab Anpfiff" : "Ballverlauf wird geladen …";
    }
    // ohne Ballaktionen bisher: Hinweis, wenn ESPN das Spiel gar nicht ueberträgt
    if (noLive(m) && !this.trail.length) this.label.textContent = NO_LIVE;
  }

  /** blasses Wappen (oder Kuerzel) mitten in der Haelfte */
  private tag(g: SVGGElement, t: SportTeam, cx: number) {
    const sig = t.logo || t.abbr;
    if (g.dataset.sig === sig) return;
    g.dataset.sig = sig;
    if (t.logo) {
      const img = sv("image", { x: cx - 13, y: H / 2 - 13, width: 26, height: 26, preserveAspectRatio: "xMidYMid meet" });
      img.setAttribute("href", smallLogo(t.logo));
      img.addEventListener("error", () => img.setAttribute("href", t.logo), { once: true });
      g.replaceChildren(img);
    } else {
      const tx = sv("text", { x: cx, y: H / 2 + 4, "text-anchor": "middle" });
      tx.textContent = t.abbr;
      g.replaceChildren(tx);
    }
  }

  get matchKey() {
    return this.key;
  }

  private clear() {
    this.queue = [];
    this.tween = null;
    this.trail = [];
    this.actors.clear();
    this.trailG.replaceChildren();
    this.shotG.replaceChildren();
    this.actorG.replaceChildren();
    this.pos = { x: W / 2, y: H / 2 };
    this.lastT = 0;
    this.place();
  }

  /**
   * Erste Ladung beim Aufklappen: aeltere Aktionen sofort hinstellen (Spieler + Spur), die letzten acht als
   * Zusammenfassung im Zeitraffer nachspielen — klappt die Notch fuer ein Tor auf, sieht man den Angriff.
   */
  reset(plays: SportPlay[]) {
    this.clear();
    const pos = plays.filter((p) => p.x >= 0);
    for (const p of pos.slice(-18, -8)) {
      this.actor(p, true);
      this.pushTrail(px(p.x), py(p.y));
      this.pos = p.x2 != null && p.y2 != null ? { x: px(p.x2), y: py(p.y2) } : { x: px(p.x), y: py(p.y) };
    }
    this.place();
    const replay = pos.slice(-8);
    this.lastT = replay[0]?.t ?? 0;
    for (const p of replay) {
      const gap = this.lastT && p.t ? p.t - this.lastT : 900;
      this.lastT = p.t || this.lastT;
      this.queue.push({ play: p, dur: Math.max(300, Math.min(1400, gap / 1.6)) });
    }
    const last = plays[plays.length - 1];
    if (last) { this.lastT = last.t; if (!replay.length) this.say(last); }
    if (this.running) this.loop();
  }

  /** Neue Aktionen: der Ball spielt sie im echten Abstand nach (bei Rueckstand schneller) */
  add(plays: SportPlay[]) {
    for (const p of plays) {
      const gap = this.lastT && p.t ? p.t - this.lastT : 1200;
      this.lastT = p.t || this.lastT;
      this.queue.push({ play: p, dur: Math.max(350, Math.min(3200, gap)) });
    }
    if (this.running) this.loop();
  }

  /** nur rechnen, solange man es sieht */
  run(on: boolean) {
    this.running = on;
    if (on) this.loop();
    else cancelAnimationFrame(this.raf);
  }

  private loop = () => {
    cancelAnimationFrame(this.raf);
    if (!this.running) return;
    const now = performance.now();
    if (this.tween) {
      const k = Math.min(1, (now - this.tween.t0) / this.tween.dur);
      const e = k < 0.5 ? 2 * k * k : 1 - (-2 * k + 2) ** 2 / 2;
      this.pos = { x: this.tween.fx + (this.tween.tx - this.tween.fx) * e, y: this.tween.fy + (this.tween.ty - this.tween.fy) * e };
      this.place();
      if (k >= 1) {
        this.pushTrail(this.pos.x, this.pos.y);
        this.tween = null;
      }
    }
    if (!this.tween && now >= this.stepEnd && this.queue.length) this.next(now);
    this.fade(now);
    if (this.tween || this.queue.length || this.actors.size) this.raf = requestAnimationFrame(this.loop);
  };

  private next(now: number) {
    const step = this.queue.shift()!;
    const p = step.play;
    // Rueckstand aufholen: je laenger die Schlange, desto schneller
    const speed = this.queue.length > 8 ? this.queue.length / 4 : 1;
    const dur = step.dur / speed;
    this.say(p);
    if (p.x < 0) { this.stepEnd = now + Math.min(dur, 900); return; }
    const sx = px(p.x), sy = py(p.y);
    this.actor(p, false);
    const hasEnd = p.x2 != null && p.y2 != null;
    const tx = hasEnd ? px(p.x2!) : sx, ty = hasEnd ? py(p.y2!) : sy;
    // Ball springt zum Ort der Aktion (kurz), dann laeuft er zum Ziel
    const jump = Math.hypot(sx - this.pos.x, sy - this.pos.y) > 1 ? Math.min(260, dur * 0.25) : 0;
    if (jump) { this.pos = { x: sx, y: sy }; this.pushTrail(sx, sy); }
    if (isShot(p.kind)) this.shot(p, sx, sy, hasEnd ? tx : undefined, hasEnd ? ty : undefined);
    if (isGoal(p.kind)) this.goal(p.side);
    this.tween = { fx: sx, fy: sy, tx, ty, t0: now, dur: Math.max(220, Math.min(1400, dur * 0.7)) };
    this.stepEnd = now + dur;
  }

  private say(p: SportPlay) {
    const team = p.side === "home" ? this.teams?.home : p.side === "away" ? this.teams?.away : undefined;
    const who = [p.jersey ? `#${p.jersey}` : "", p.who].filter(Boolean).join(" ");
    this.label.replaceChildren(
      mk("span", "pl-min", p.minute),
      team ? crestEl(team, "crest pl-crest") : mk("i", `pl-dot ${p.side}`),
      mk("span", "pl-who", who),
      mk("span", "pl-what", playName(p.kind)),
    );
  }

  /** Spieler mit Rueckennummer an seiner letzten Position; verblasst nach einer Weile */
  private actor(p: SportPlay, instant: boolean) {
    if (!p.jersey || !p.side) return;
    const id = `${p.side}-${p.jersey}`;
    let a = this.actors.get(id);
    if (!a) {
      const g = sv("g", { class: `actor ${p.side}` });
      const c = this.colors[p.side as "home" | "away"] ?? "#fff";
      g.append(sv("circle", { r: 2.7, fill: c }));
      const t = sv("text", { y: 0.95, "text-anchor": "middle", fill: lum(c) > 150 ? "#000" : "#fff" });
      t.textContent = p.jersey;
      g.append(t);
      this.actorG.append(g);
      a = { g, at: 0 };
      this.actors.set(id, a);
    }
    a.at = instant ? performance.now() - 6000 : performance.now();
    // CSS-Transform (Einheiten = Spielfeld-Meter): so gleitet der Spieler zu seiner neuen Position
    a.g.style.transition = instant ? "none" : "";
    a.g.style.transform = `translate(${px(p.x).toFixed(2)}px, ${py(p.y).toFixed(2)}px)`;
    a.g.style.opacity = "1";
    this.actorG.append(a.g); // nach oben
  }

  private fade(now: number) {
    for (const [id, a] of this.actors) {
      const age = (now - a.at) / 1000;
      const o = age < 8 ? 1 : Math.max(0, 1 - (age - 8) / 14);
      a.g.style.opacity = o.toFixed(2);
      if (o <= 0) { a.g.remove(); this.actors.delete(id); }
    }
  }

  private pushTrail(x: number, y: number) {
    this.trail.push({ x, y });
    if (this.trail.length > 9) this.trail.shift();
    const segs: SVGLineElement[] = [];
    for (let i = 1; i < this.trail.length; i++) {
      const a = this.trail[i - 1], b = this.trail[i];
      segs.push(sv("line", { x1: a.x.toFixed(2), y1: a.y.toFixed(2), x2: b.x.toFixed(2), y2: b.y.toFixed(2), opacity: ((i / this.trail.length) * 0.55).toFixed(2) }));
    }
    this.trailG.replaceChildren(...segs);
  }

  private shot(p: SportPlay, x: number, y: number, tx?: number, ty?: number) {
    // Ziel: angegebene Stelle oder Tormitte des Gegners
    const gx = tx ?? (p.side === "away" ? 0 : W), gy = ty ?? H / 2;
    const c = this.colors[p.side as "home" | "away"] ?? "#fff";
    const l = sv("line", { x1: x, y1: y, x2: gx, y2: gy, stroke: c, class: "shot" + (isGoal(p.kind) ? " goal" : "") });
    this.shotG.append(l);
    window.setTimeout(() => l.remove(), 5000);
  }

  private goal(side: string) {
    const n = side === "away" ? this.net.l : this.net.r;
    n.classList.remove("hit"); void n.getBoundingClientRect(); n.classList.add("hit");
    this.el.classList.remove("goal"); void this.el.offsetWidth; this.el.classList.add("goal");
  }

  private place() {
    this.ball.setAttribute("transform", `translate(${this.pos.x.toFixed(2)} ${this.pos.y.toFixed(2)})`);
  }
}

// ---------- Basketball: Wurfbild (Abschluesse, Treffer und Fehlwuerfe) ----------
//
// ESPN liefert jeden Wurf mit Ort in Fuss (x quer 0..50, y laengs, Korb bei y ≈ 1; gegen die Distanzen im
// Spieltext geprueft) — fuer beide Teams auf "ihren" Korb gerechnet. Hier auf einem ganzen Feld (94 × 50 ft):
// Heim wirft rechts, Gast links (wie beim Fussball: Heim spielt nach rechts). Treffer gefuellt in der Teamfarbe,
// Fehlwuerfe als Kreuz; der letzte Wurf leuchtet auf. Freiwuerfe haben keinen Ort und zaehlen nur in der Bilanz.

const CW = 94, CH = 50;
/** Abstand Grundlinie–Korbmitte (Regel 5,25 ft); ESPN-Korb bei y = 1 */
const RIM = 5.25, ESPN_RIM_Y = 1;

type Tally = { fg: [number, number]; three: [number, number]; ft: [number, number] };
const emptyTally = (): Tally => ({ fg: [0, 0], three: [0, 0], ft: [0, 0] });
const pct = (a: [number, number]) => (a[1] ? `${Math.round((a[0] / a[1]) * 100)} %` : "–");

export class Court {
  readonly el: HTMLElement;
  private svg: SVGSVGElement;
  private shotG: SVGGElement;
  private tagH: SVGGElement;
  private tagA: SVGGElement;
  private label: HTMLElement;
  private tally: HTMLElement;
  private key = "";
  private teams: { home: SportTeam; away: SportTeam } | null = null;
  private shots = new Map<string, SportPlay>();
  private compact: boolean;

  /** compact: Bilanz in einer Zeile je Team (Notch); sonst ausfuehrlich (Arena) */
  constructor(compact = false) {
    this.compact = compact;
    this.el = mk("div", "court");
    this.el.dataset.key = "court";
    this.el.dataset.keep = "";
    this.svg = sv("svg", { viewBox: `-2 -2 ${CW + 4} ${CH + 4}`, class: "court-svg", "aria-hidden": "true" });
    const lines = sv("g", { class: "cl" });
    lines.append(
      sv("rect", { x: 0, y: 0, width: CW, height: CH, rx: 0.6 }),
      sv("line", { x1: CW / 2, y1: 0, x2: CW / 2, y2: CH }),
      sv("circle", { cx: CW / 2, cy: CH / 2, r: 6 }),
      sv("circle", { cx: CW / 2, cy: CH / 2, r: 2 }),
    );
    for (const right of [false, true]) {
      const X = (x: number) => (right ? CW - x : x);
      const sweep = right ? 0 : 1;
      lines.append(
        sv("rect", { x: right ? CW - 19 : 0, y: 17, width: 19, height: 16, class: "paint" }),
        sv("circle", { cx: X(19), cy: CH / 2, r: 6 }),
        sv("path", { d: `M${X(0)} 3 L${X(14.2)} 3 A23.75 23.75 0 0 ${sweep} ${X(14.2)} 47 L${X(0)} 47` }),
        sv("path", { d: `M${X(RIM)} 21 A4 4 0 0 ${sweep} ${X(RIM)} 29` }),
        sv("line", { x1: X(4), y1: 22, x2: X(4), y2: 28, class: "board" }),
        sv("circle", { cx: X(RIM), cy: CH / 2, r: 0.75, class: "rim" }),
      );
    }
    // Wappen blass in der Haelfte, in der die Wuerfe des Teams landen
    this.tagH = sv("g", { class: "court-tag h" });
    this.tagA = sv("g", { class: "court-tag a" });
    this.shotG = sv("g", { class: "court-shots" });
    this.svg.append(lines, this.tagH, this.tagA, this.shotG);
    this.label = mk("div", "pitch-label court-label n-liquid", "Würfe werden geladen …");
    const field = mk("div", "court-field");
    field.append(this.svg, this.label);
    this.tally = mk("div", "court-tally");
    this.el.append(field, this.tally);
  }

  get matchKey() {
    return this.key;
  }

  setMatch(m: SportMatch) {
    this.teams = { home: m.home, away: m.away };
    this.el.style.setProperty("--hc", m.home.color);
    this.el.style.setProperty("--ac", m.away.color);
    this.tag(this.tagH, m.home, (CW * 3) / 4);
    this.tag(this.tagA, m.away, CW / 4);
    if (m.key !== this.key) {
      this.key = m.key;
      this.shots.clear();
      this.draw();
      this.label.textContent = m.state === "pre" ? "Würfe ab Spielbeginn" : "Würfe werden geladen …";
    }
  }

  private tag(g: SVGGElement, t: SportTeam, cx: number) {
    const sig = t.logo || t.abbr;
    if (g.dataset.sig === sig) return;
    g.dataset.sig = sig;
    if (t.logo) {
      const img = sv("image", { x: cx - 10, y: CH / 2 - 10, width: 20, height: 20, preserveAspectRatio: "xMidYMid meet" });
      img.setAttribute("href", smallLogo(t.logo));
      g.replaceChildren(img);
    } else {
      const tx = sv("text", { x: cx, y: CH / 2 + 3, "text-anchor": "middle" });
      tx.textContent = t.abbr;
      g.replaceChildren(tx);
    }
  }

  /** alle Wuerfe des Spiels (erste Ladung) */
  reset(plays: SportPlay[]) {
    this.shots.clear();
    this.add(plays, false);
  }

  /** neue Wuerfe: nur sie kommen dazu, der letzte leuchtet auf */
  add(plays: SportPlay[], fresh = true) {
    for (const p of plays) if (p.kind === "2" || p.kind === "3" || p.kind === "ft") this.shots.set(p.id, p);
    this.draw(fresh ? plays.filter((p) => this.shots.has(p.id)).map((p) => p.id) : []);
  }

  /** ESPN-Ort -> Feld: Heim wirft rechts, Gast links */
  private spot(p: SportPlay): [number, number] | null {
    if (p.x < 0 || p.x > 50 || p.y < -6 || p.y > 90) return null;
    const d = p.y - ESPN_RIM_Y + RIM; // Abstand zur Grundlinie
    return p.side === "home" ? [CW - d, CH - p.x] : [d, p.x];
  }

  private draw(fresh: string[] = []) {
    const color = (side: string) => (side === "home" ? this.teams?.home.color : this.teams?.away.color) ?? "#fff";
    // Treffer und Fehlwuerfe als feste Elemente je Wurf: vorhandene bleiben stehen, neue kommen dazu
    const have = new Map<string, SVGGElement>();
    for (const g of Array.from(this.shotG.children) as SVGGElement[]) have.set(g.dataset.id ?? "", g);
    const keep = new Set<string>();
    for (const p of this.shots.values()) {
      const at = this.spot(p);
      if (!at) continue;
      keep.add(p.id);
      if (have.has(p.id)) continue;
      const g = sv("g", { class: `shot ${p.made ? "made" : "miss"} ${p.side}` });
      g.dataset.id = p.id;
      g.setAttribute("transform", `translate(${at[0].toFixed(2)} ${at[1].toFixed(2)})`);
      const c = color(p.side);
      if (p.made) g.append(sv("circle", { r: 0.85, fill: c }));
      else g.append(sv("path", { d: "M-0.65 -0.65 L0.65 0.65 M0.65 -0.65 L-0.65 0.65", stroke: c }));
      this.shotG.append(g);
    }
    for (const [id, g] of have) if (!keep.has(id)) g.remove();
    for (const id of fresh) {
      const g = this.shotG.querySelector<SVGGElement>(`[data-id="${CSS.escape(id)}"]`);
      if (g) { g.classList.remove("new"); void g.getBoundingClientRect(); g.classList.add("new"); }
    }
    this.renderTally();
    const last = [...this.shots.values()].pop();
    if (last) this.say(last);
    else if (this.key) this.label.textContent = "Noch keine Würfe";
  }

  private say(p: SportPlay) {
    const team = p.side === "home" ? this.teams?.home : p.side === "away" ? this.teams?.away : undefined;
    const what = p.kind === "ft" ? "Freiwurf" : p.kind === "3" ? "Dreier" : "Wurf";
    this.label.replaceChildren(
      mk("span", "pl-min", p.minute),
      team ? crestEl(team, "crest pl-crest") : mk("i", "pl-dot"),
      mk("span", "pl-who", p.who),
      mk("span", `pl-what ${p.made ? "hit" : "miss"}`, `${what} ${p.made ? "✓" : "✗"}`),
    );
  }

  private renderTally() {
    const t = { home: emptyTally(), away: emptyTally() };
    for (const p of this.shots.values()) {
      const s = t[p.side as "home" | "away"];
      if (!s) continue;
      const add = (a: [number, number]) => { a[1]++; if (p.made) a[0]++; };
      if (p.kind === "ft") add(s.ft);
      else {
        add(s.fg);
        if (p.kind === "3") add(s.three);
      }
    }
    const row = (side: "home" | "away") => {
      const team = side === "home" ? this.teams?.home : this.teams?.away;
      const s = t[side];
      const r = mk("div", `ct-row ${side}`);
      r.dataset.key = side;
      if (team) r.append(crestEl(team, "crest"));
      const cell = (label: string, a: [number, number], withPct: boolean) => {
        const c = mk("span", "ct-cell");
        c.append(mk("small", "", label), mk("b", "", `${a[0]}/${a[1]}`));
        if (withPct) c.append(mk("i", "", pct(a)));
        return c;
      };
      r.append(cell("Würfe", s.fg, true), cell("Dreier", s.three, !this.compact), cell("Freiwürfe", s.ft, !this.compact));
      return r;
    };
    // Gast links wie auf dem Feld
    morph(this.tally, [row("away"), row("home")]);
  }
}
