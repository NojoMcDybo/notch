/**
 * LoL-Profispiele in der Notch — Daten kommen fertig von Vantage (Activity mit Feld `esports`, esnotch.rs).
 *
 * - Klein (Mitte der Notch): Logo – Gold – Spielzeit – Gold – Logo, wenn die Notch sonst leer ist; teilt sie sich den
 *   Platz, nur der Goldvorsprung als positive Zahl auf der Seite des Führenden. Draft: Logos und „Draft“.
 * - Aufgeklappt, wie in der Übertragung: Kopfleiste mit Logo, Kürzel, Türmen und Gold je Team (gespiegelt), Kills in
 *   der Mitte; darunter Baron, Inhibitoren, Drachen und die Spielzeit; dann je Lane Level mit Schlüsselrune, Items,
 *   K/D/A und CS und das Champion-Bild (weich auslaufend, zur Mitte knapp beschnitten), in der Mitte der
 *   Goldvorsprung der Lane auf der Seite des Führenden; unten der Goldverlauf.
 * - Klick auf die Karte öffnet das Spiel in Vantage (vantage://…, steht in der Activity).
 *
 * Bilder nur von Data Dragon und lolesports (Champions, Items, Runen, Logos) — andere Adressen werden ignoriert.
 */

import "@fontsource/cinzel/700.css";
import "@fontsource/saira-semi-condensed/500.css";
import "@fontsource/saira-semi-condensed/600.css";
import "@fontsource/saira-semi-condensed/700.css";
import "@fontsource-variable/source-sans-3";
import { LOL, LOL_MASK, type LolIcon } from "./lolicons";

export type EsTeam = {
  code: string; name: string; logo: string; wins?: number;
  gold?: number; kills?: number; towers?: number; inhibs?: number; barons?: number; dragons?: string[];
};
export type EsPlayer = {
  name: string; champ: string; art: string; icon: string; level: number;
  k: number; d: number; a: number; cs: number; gold: number; items: string[]; rune: string;
};
export type EsLane = { role: string; b: EsPlayer; r: EsPlayer; diff: number };
export type Esports = {
  mode: "game" | "post" | "draft";
  league: string; block: string; bo: number; game: number;
  clock?: number; paused?: boolean; winner?: string;
  blue: EsTeam; red: EsTeam;
  lanes?: EsLane[]; gold?: number[];
};

function mk<K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
}

const OK_IMG = /^https:\/\/(ddragon\.leagueoflegends\.com|static\.lolesports\.com|am-a\.akamaihd\.net)\//;
const safe = (u: string | undefined) => (u && OK_IMG.test(u) ? u : "");

function img(src: string, cls: string, alt = "") {
  const s = safe(src);
  if (!s) return mk("span", `${cls} none`);
  const i = new Image();
  i.className = cls;
  i.alt = alt;
  i.draggable = false;
  i.decoding = "async";
  i.src = s;
  i.onerror = () => i.classList.add("none");
  return i;
}

// ---------- Symbole (Original aus dem Spiel, lolicons.ts) ----------

/** Teamfarben der Symbole wie in der Übertragung */
export const ES_BLUE = "#6f9bff", ES_GOLD = "#d9b46a";
let uid = 0;
/** Drachen farbig; Türme, Inhibitoren, Baron, Gold und Kills als Maske in `color` */
export function ic(key: string, color = "") {
  const s = mk("span", "es-ic");
  const k = (key === "gold" ? "goldm" : key === "kills" ? "killsm" : key in LOL ? key : "dragon") as LolIcon;
  if (LOL_MASK.has(k)) {
    s.classList.add("m");
    s.style.setProperty("--m", `url("${LOL[k]}")`);
    s.style.color = color || ES_GOLD;
  } else s.style.backgroundImage = `url("${LOL[k]}")`;
  return s;
}

// ---------- Zahlen ----------

export const esClock = (t = 0) => `${Math.floor(t / 60)}:${String(Math.floor(t % 60)).padStart(2, "0")}`;
const k = (n = 0) => `${(n / 1000).toFixed(1).replace(".", ",")}K`;
const lead = (d: Esports) => (d.blue.gold ?? 0) - (d.red.gold ?? 0);
const showScore = (d: Esports) => d.blue.wins != null && d.red.wins != null;

/** Unterschrift fürs Neuzeichnen: nur wenn sich etwas Sichtbares ändert */
export function esSig(d: Esports, full: boolean) {
  return JSON.stringify([d.mode, d.clock, d.paused, d.winner, d.blue, d.red, d.lanes, d.gold?.[d.gold.length - 1], d.gold?.length, full]);
}

// ---------- Klein ----------

/** Mitte der kleinen Notch; `full` = die Notch gehört ihr allein (dann Gold beider Teams) */
export function esMid(d: Esports, full: boolean) {
  const box = mk("div", `es-mid ${full ? "full" : "shared"} ${d.mode}`);
  const side = (t: EsTeam, s: "b" | "r") => {
    const w = mk("span", `es-m-side ${s}`);
    w.append(img(t.logo, "es-logo", t.code));
    if (d.mode === "draft") return w;
    if (full) {
      const v = mk("span", "es-m-gold");
      v.append(ic("gold", s === "b" ? ES_BLUE : ES_GOLD), mk("span", "", k(t.gold)));
      w.append(v);
    } else {
      const l = lead(d);
      if ((s === "b" && l > 0) || (s === "r" && l < 0)) w.append(mk("span", `es-m-lead ${s}`, `+${k(Math.abs(l))}`));
    }
    return w;
  };
  let center: HTMLElement;
  if (d.mode === "draft") center = mk("span", "es-m-draft", showScore(d) ? `Draft · ${d.blue.wins}–${d.red.wins}` : "Draft");
  else if (d.mode === "post") center = mk("span", "es-m-clock end", d.winner ? `Sieg ${d.winner === "blue" ? d.blue.code : d.red.code}` : "Ende");
  else center = mk("span", `es-m-clock${d.paused ? " paused" : ""}`, d.paused ? "Pause" : esClock(d.clock));
  box.append(side(d.blue, "b"), center, side(d.red, "r"));
  return box;
}

// ---------- Aufgeklappt ----------

function bar(d: Esports) {
  const b = mk("div", "es-bar");
  const side = (t: EsTeam, s: "b" | "r") => {
    const w = mk("div", `es-side ${s}`);
    w.append(img(t.logo, "es-logo", t.code), mk("span", "es-code", t.code), mk("span", "es-gap"));
    if (d.mode !== "draft") {
      const tw = mk("span", "es-stat tw");
      tw.append(ic("tower", s === "b" ? ES_BLUE : ES_GOLD), mk("span", "", String(t.towers ?? 0)));
      const gd = mk("span", "es-stat gd");
      gd.append(ic("gold", s === "b" ? ES_BLUE : ES_GOLD), mk("span", "", k(t.gold)));
      w.append(tw, gd);
    }
    return w;
  };
  const mid = mk("div", "es-mid-k");
  if (d.mode === "draft") {
    mid.classList.add("series");
    mid.append(mk("span", "", `Spiel ${d.game}`));
    if (showScore(d)) mid.append(mk("b", "", `${d.blue.wins} – ${d.red.wins}`));
    mid.append(mk("span", "", `Bo${d.bo}`));
  } else {
    mid.append(mk("b", "", String(d.blue.kills ?? 0)), ic("kills", ES_BLUE), ic("kills", ES_GOLD), mk("b", "", String(d.red.kills ?? 0)));
  }
  b.append(side(d.blue, "b"), mid, side(d.red, "r"));
  return b;
}

function sub(d: Esports) {
  const row = mk("div", "es-sub");
  const side = (t: EsTeam, s: "b" | "r") => {
    const w = mk("div", `es-side ${s}`);
    const st = (key: string, n = 0) => {
      const x = mk("span", "es-stat");
      x.append(ic(key, s === "b" ? ES_BLUE : ES_GOLD), mk("span", "", String(n)));
      return x;
    };
    const dr = mk("span", "es-drakes");
    for (const t2 of t.dragons ?? []) dr.append(ic(t2));
    w.append(st("baron", t.barons), st("inhib", t.inhibs), mk("span", "es-gap"), dr);
    return w;
  };
  const c = d.mode === "post"
    ? mk("span", "es-clock end", d.winner ? `Sieg ${d.winner === "blue" ? d.blue.code : d.red.code}` : "Ende")
    : mk("span", `es-clock${d.paused ? " paused" : ""}`, d.paused ? `Pause · ${esClock(d.clock)}` : esClock(d.clock));
  row.append(side(d.blue, "b"), c, side(d.red, "r"));
  return row;
}

function lane(l: EsLane) {
  const row = mk("div", "es-row");
  const items = (p: EsPlayer, s: "b" | "r") => {
    const w = mk("span", `es-items ${s}`);
    const im = p.items.slice(0, 6).map((u) => img(u, "es-item"));
    const empty = Array.from({ length: 6 - im.length }, () => mk("i", "es-item none"));
    w.append(...(s === "b" ? [...im, ...empty] : [...empty, ...im.reverse()]));
    return w;
  };
  const lvl = (p: EsPlayer) => {
    const w = mk("span", "es-lvl");
    w.append(img(p.rune, "es-rune"), mk("b", "", String(p.level)));
    return w;
  };
  const kda = (p: EsPlayer, s: "b" | "r") => {
    const w = mk("span", `es-kda ${s}`);
    w.append(mk("b", "", `${p.k}/${p.d}/${p.a}`), mk("span", "", `${p.cs} CS`));
    return w;
  };
  const art = (p: EsPlayer, s: "b" | "r") => {
    const w = mk("span", `es-art ${s}`);
    const i = mk("i");
    const src = safe(p.art);
    if (src) i.style.backgroundImage = `url("${src}")`;
    w.append(i);
    return w;
  };
  const diff = mk("span", `es-diff ${l.diff > 0 ? "b" : l.diff < 0 ? "r" : "even"}`);
  diff.append(mk("span", "", l.diff === 0 ? "±0" : `+${k(Math.abs(l.diff))}`));
  row.append(
    lvl(l.b), items(l.b, "b"), kda(l.b, "b"), art(l.b, "b"), mk("span", "es-name b", l.b.name),
    diff,
    art(l.r, "r"), kda(l.r, "r"), items(l.r, "r"), lvl(l.r), mk("span", "es-name r", l.r.name),
  );
  return row;
}

/** Goldverlauf: blau über der Linie, gold darunter */
function goldLine(pts: number[]) {
  // Nulllinie dort, wo es passt: führt nur ein Team, bekommt es fast die ganze Höhe
  const W = 480, H = 36;
  const up = Math.max(0, ...pts), down = Math.max(0, ...pts.map((v) => -v));
  const scale = Math.max(1000, up + down);
  const mid = Math.min(H - 4, Math.max(4, 2 + (up / scale) * (H - 4)));
  const x = (i: number) => (pts.length < 2 ? W : (i / (pts.length - 1)) * W);
  const y = (v: number) => mid - (v / scale) * (H - 4);
  const path = pts.map((v, i) => `${i ? "L" : "M"}${x(i).toFixed(1)} ${y(v).toFixed(1)}`).join("");
  const area = `${path}L${W} ${mid}L0 ${mid}Z`;
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", `0 0 ${W} ${H}`);
  svg.setAttribute("preserveAspectRatio", "none");
  svg.setAttribute("class", "es-goldline");
  const id = `esl${++uid}`;
  svg.innerHTML = `<defs><clipPath id="${id}a"><rect x="0" y="0" width="${W}" height="${mid}"/></clipPath><clipPath id="${id}b"><rect x="0" y="${mid}" width="${W}" height="${H - mid}"/></clipPath></defs>
    <path d="${area}" fill="rgba(61,139,255,0.55)" clip-path="url(#${id}a)"/><path d="${area}" fill="rgba(227,189,109,0.5)" clip-path="url(#${id}b)"/>
    <line x1="0" y1="${mid}" x2="${W}" y2="${mid}" stroke="rgba(255,255,255,0.1)"/>`;
  return svg;
}

/** Karte in der aufgeklappten Notch; Klick öffnet das Spiel in Vantage */
export function esCard(d: Esports, onOpen: () => void) {
  const card = mk("div", `es ${d.mode}`);
  card.title = "In Vantage öffnen";
  card.onclick = onOpen;
  card.append(bar(d));
  if (d.mode === "draft") {
    const body = mk("div", "es-draft");
    body.append(mk("b", "", `Draft läuft · Spiel ${d.game}`), mk("span", "", "Champions und Scoreboard erscheinen, sobald das Spiel beginnt."));
    card.append(body);
  } else {
    card.append(sub(d));
    const rows = mk("div", "es-rows");
    for (const l of d.lanes ?? []) rows.append(lane(l));
    card.append(rows);
    if ((d.gold?.length ?? 0) > 1) {
      const gl = mk("div", "es-gl");
      const l = lead(d);
      gl.append(goldLine(d.gold!), mk("span", `es-gl-v ${l > 0 ? "b" : l < 0 ? "r" : ""}`, l === 0 ? "±0" : `${l > 0 ? d.blue.code : d.red.code} +${k(Math.abs(l))}`));
      card.append(gl);
    }
  }
  const foot = mk("div", "es-foot");
  foot.append(mk("span", "", [d.league, d.block].filter(Boolean).join(" · ")), mk("span", "", showScore(d) ? `Serie ${d.blue.wins}–${d.red.wins} · Bo${d.bo}` : `Spiel ${d.game} · Bo${d.bo}`));
  card.append(foot);
  return card;
}

/** für den Sprachassistenten: alles Wichtige als Klartext-Daten */
export function esVoice(d: Esports) {
  const t = (x: EsTeam) => ({
    team: x.name || x.code, kuerzel: x.code, gold: x.gold, kills: x.kills, tuerme: x.towers, inhibitoren: x.inhibs, barone: x.barons, drachen: x.dragons,
    siege_in_der_serie: x.wins,
  });
  return {
    phase: d.mode === "draft" ? "Draft (Spiel hat noch nicht begonnen)" : d.mode === "post" ? "Spiel vorbei" : d.paused ? "pausiert" : "läuft",
    liga: d.league, runde: d.block, spiel: d.game, best_of: d.bo,
    spielzeit: d.mode === "draft" ? null : esClock(d.clock),
    sieger: d.winner ? (d.winner === "blue" ? d.blue.code : d.red.code) : null,
    blau: t(d.blue), rot: t(d.red),
    lanes: (d.lanes ?? []).map((l) => ({
      rolle: l.role,
      blau: `${l.b.name} (${l.b.champ}) ${l.b.k}/${l.b.d}/${l.b.a}, ${l.b.cs} CS, Level ${l.b.level}`,
      rot: `${l.r.name} (${l.r.champ}) ${l.r.k}/${l.r.d}/${l.r.a}, ${l.r.cs} CS, Level ${l.r.level}`,
      goldvorsprung: l.diff === 0 ? "gleich" : `${l.diff > 0 ? d.blue.code : d.red.code} +${Math.abs(l.diff)}`,
    })),
    hinweis: "Die Daten liegen etwa eine Minute hinter der Übertragung.",
  };
}
