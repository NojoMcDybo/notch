/**
 * Live-Sport in der Notch (Daten: src-tauri/src/sport.rs).
 *
 * - Mitte der kleinen Notch: Spielstand mit Kuerzeln und Minute; nach einem Tor leuchtet er kurz in der Teamfarbe.
 * - Neue Meldung: die Notch klappt kurz auf, die Meldung steht ganz oben unter der Leiste.
 * - Aufgeklappt: Karte mit Spielstand, Spielfeld mit Ballverlauf, Ticker und weitere laufende Spiele.
 *
 * Ballverlauf: echte Positionsdaten aller Spieler gibt es live nicht frei. Die Notch spielt stattdessen jede
 * Ballaktion (Pass von wo nach wo, Flanke, Schuss, Zweikampf …) im Takt der echten Uhrzeit nach: der Ball
 * wandert von Aktion zu Aktion, die beteiligten Spieler erscheinen mit Rueckennummer in ihrer Teamfarbe.
 */

export type SportTeam = { id: string; name: string; short: string; abbr: string; logo: string; color: string; score: string };
export type SportEv = { id: string; minute: string; kind: string; side: string; title: string; text: string; big: number };
export type SportMatch = {
  key: string; league: string; league_name: string; sport: string; home: SportTeam; away: SportTeam;
  state: "pre" | "in" | "post"; clock: string; start: number; fav: boolean; link: string; pitch: boolean;
  events: SportEv[]; source: string;
};
export type SportState = { matches: SportMatch[]; error?: string; updated?: number; off?: boolean };
export type SportNews = { key: string; ev: SportEv; score: string; at: number };
export type SportPlay = {
  id: string; t: number; minute: string; kind: string; side: string; jersey: string; who: string;
  x: number; y: number; x2?: number | null; y2?: number | null;
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

/** eigene Symbole statt Emoji (Emoji sehen je nach Windows-Version anders aus) */
const BALL_SVG = `<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="10" fill="#fff"/><path fill="#111" d="m12 7.2 3.4 2.5-1.3 4h-4.2l-1.3-4zM12 2.2v3l-3.3 2.4-2.9-.9a10 10 0 0 1 6.2-4.5zm0 0a10 10 0 0 1 6.2 4.5l-2.9.9L12 5.2zM2.3 10.4l2.8.9 1.3 3.9-1.8 2.4a9.9 9.9 0 0 1-2.3-7.2zm19.4 0a9.9 9.9 0 0 1-2.3 7.2l-1.8-2.4 1.3-3.9zM8.6 21.4l.1-3h6.6l.1 3a10 10 0 0 1-6.8 0z"/></svg>`;
const SPORT_GLYPH: Record<string, string> = { hockey: "●", football: "◆", basketball: "●", baseball: "●" };

/** Anstoss: "20:30" heute, sonst "Sa 15:30" */
export function kickoff(m: SportMatch) {
  const d = new Date(m.start);
  const t = d.toLocaleTimeString("de-DE", { hour: "2-digit", minute: "2-digit" });
  const today = new Date().toDateString() === d.toDateString();
  return today ? t : `${d.toLocaleDateString("de-DE", { weekday: "short" }).replace(".", "")} ${t}`;
}

const scoreOf = (m: SportMatch) => (m.state === "pre" ? "–" : `${m.home.score || 0}:${m.away.score || 0}`);
const clockOf = (m: SportMatch) => (m.state === "pre" ? kickoff(m) : m.clock || (m.state === "post" ? "Ende" : ""));

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

export function midEl(m: SportMatch, hot: SportNews | null) {
  const box = mk("div", `sm ${m.state}` + (hot ? " hot" : ""));
  box.style.setProperty("--hc", m.home.color);
  box.style.setProperty("--ac", m.away.color);
  if (hot) box.style.setProperty("--hot", newsColor(hot, m));
  box.title = `${m.home.name} – ${m.away.name} · ${m.league_name}`;
  const h = mk("b", "sm-t h", m.home.abbr || m.home.short.slice(0, 3).toUpperCase());
  const a = mk("b", "sm-t a", m.away.abbr || m.away.short.slice(0, 3).toUpperCase());
  const sc = mk("span", "sm-s", scoreOf(m));
  const cl = mk("small", "sm-c", clockOf(m));
  box.append(h, sc, a, cl);
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
  const b = mk("div", `sp-news k-${n.ev.kind}` + (n.ev.big >= 3 ? " big" : ""));
  b.style.setProperty("--c", newsColor(n, m));
  b.append(kindIcon(n.ev.kind, m?.sport ?? "soccer"));
  const body = mk("div", "spn-body");
  body.append(mk("b", "", n.ev.title));
  const sub = [n.ev.text, n.ev.minute].filter(Boolean).join(" · ");
  if (sub) body.append(mk("span", "", sub));
  b.append(body);
  if (m) {
    const sc = mk("div", "spn-score");
    sc.append(mk("span", "", m.home.abbr), mk("b", "", n.score || scoreOf(m)), mk("span", "", m.away.abbr));
    b.append(sc);
  }
  return b;
}

// ---------- Karte (aufgeklappt) ----------

function logo(t: SportTeam) {
  const box = mk("span", "sp-logo");
  box.style.setProperty("--c", t.color);
  if (t.logo) {
    const img = new Image();
    img.decoding = "async";
    img.loading = "lazy";
    img.src = t.logo;
    img.alt = "";
    img.onerror = () => { img.remove(); box.textContent = t.abbr.slice(0, 3); };
    box.append(img);
  } else box.textContent = t.abbr.slice(0, 3);
  return box;
}

export type CardOpts = {
  pitch: Pitch | null;
  onOpen: (m: SportMatch) => void;
  onFocus: (key: string) => void;
  side: boolean;
};

/** Ticker: neueste Meldung oben, hoechstens n Zeilen */
function tickerEl(m: SportMatch, n: number) {
  const ol = mk("ol", "sp-ticker");
  const evs = [...m.events].reverse().slice(0, n);
  if (!evs.length) {
    ol.append(mk("li", "sp-empty", m.state === "pre" ? `Anstoß ${kickoff(m)}` : m.source === "openligadb" ? "Tore erscheinen hier (OpenLigaDB)" : "Noch keine Meldungen"));
    return ol;
  }
  for (const e of evs) {
    const li = mk("li", `k-${e.kind}` + (e.big >= 3 ? " big" : ""));
    li.style.setProperty("--c", e.side === "away" ? m.away.color : e.side === "home" ? m.home.color : "var(--fg2)");
    li.append(mk("span", "tk-m", e.minute), kindIcon(e.kind, m.sport));
    const t = mk("span", "tk-t");
    t.append(mk("b", "", e.title));
    if (e.text) t.append(" ", mk("span", "", e.text));
    li.append(t);
    ol.append(li);
  }
  return ol;
}

/** Signatur der Karte: nur neu bauen, wenn sich Sichtbares aendert (der Ballverlauf lebt weiter) */
export function cardSig(focus: SportMatch | undefined, others: SportMatch[], side: boolean, pitchOn: boolean) {
  const f = focus ? [focus.key, focus.home.score, focus.away.score, focus.clock, focus.state, focus.events.map((e) => e.id).join(",")] : [];
  return JSON.stringify([f, others.map((m) => [m.key, m.home.score, m.away.score, m.clock]), side, pitchOn]);
}

export function cardEl(focus: SportMatch, others: SportMatch[], o: CardOpts) {
  const wrap = mk("section", "sport");
  const card = mk("div", `sp-card ${focus.state}`);
  card.style.setProperty("--hc", focus.home.color);
  card.style.setProperty("--ac", focus.away.color);

  const head = mk("div", "sp-head");
  head.append(mk("span", "sp-league", focus.league_name));
  const clock = mk("span", `sp-clock ${focus.state}`, clockOf(focus));
  head.append(clock);
  const open = mk("button", "sp-open", "Spielseite");
  open.title = "Spiel im Browser öffnen";
  open.hidden = !focus.link;
  open.addEventListener("click", (e) => { e.stopPropagation(); o.onOpen(focus); });
  head.append(open);

  const score = mk("div", "sp-score");
  const th = mk("div", "sp-team h");
  th.append(logo(focus.home), mk("span", "sp-name", focus.home.name));
  const ta = mk("div", "sp-team a");
  ta.append(mk("span", "sp-name", focus.away.name), logo(focus.away));
  const num = mk("div", "sp-num");
  if (focus.state === "pre") num.append(mk("span", "sp-ko", kickoff(focus)));
  else num.append(mk("b", "", focus.home.score || "0"), mk("i", "", ":"), mk("b", "", focus.away.score || "0"));
  score.append(th, num, ta);
  card.append(head, score);

  const body = mk("div", "sp-body" + (o.pitch ? " with-pitch" : ""));
  if (o.pitch) body.append(o.pitch.el);
  body.append(tickerEl(focus, o.pitch ? (o.side ? 3 : 5) : 4));
  card.append(body);
  wrap.append(card);

  if (others.length) {
    const more = mk("div", "sp-more");
    for (const m of others.slice(0, 6)) {
      const c = mk("button", `sp-chip ${m.state}`);
      c.title = `${m.home.name} – ${m.away.name} · ${m.league_name}`;
      c.append(mk("b", "", m.home.abbr), mk("span", "", scoreOf(m)), mk("b", "", m.away.abbr), mk("small", "", clockOf(m)));
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
  private tagH: SVGTextElement;
  private tagA: SVGTextElement;
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
    // Kuerzel der Teams blass im Hintergrund ihrer Haelfte (Heim links, spielt nach rechts)
    this.tagH = sv("text", { x: W / 4, y: H / 2 + 4, "text-anchor": "middle", class: "pitch-tag h" });
    this.tagA = sv("text", { x: (W * 3) / 4, y: H / 2 + 4, "text-anchor": "middle", class: "pitch-tag a" });
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
    this.label = mk("div", "pitch-label", "Ballverlauf wird geladen …");
    this.el.append(this.svg, this.label);
    this.place();
  }

  /** Spiel wechseln: Farben uebernehmen, bei anderem Spiel alles leeren */
  setMatch(m: SportMatch) {
    this.colors = { home: m.home.color, away: m.away.color };
    this.el.style.setProperty("--hc", m.home.color);
    this.el.style.setProperty("--ac", m.away.color);
    this.el.dataset.h = m.home.abbr;
    this.el.dataset.a = m.away.abbr;
    this.tagH.textContent = m.home.abbr;
    this.tagA.textContent = m.away.abbr;
    if (m.key !== this.key) {
      this.key = m.key;
      this.clear();
      this.label.textContent = m.state === "pre" ? "Ballverlauf ab Anpfiff" : "Ballverlauf wird geladen …";
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
    const team = p.side === "home" ? this.el.dataset.h : p.side === "away" ? this.el.dataset.a : "";
    const who = [p.jersey ? `#${p.jersey}` : "", p.who].filter(Boolean).join(" ");
    this.label.replaceChildren(
      mk("span", "pl-min", p.minute),
      mk("i", `pl-dot ${p.side}`),
      mk("span", "pl-who", [team, who].filter(Boolean).join(" · ")),
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
