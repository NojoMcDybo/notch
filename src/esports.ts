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

// ---------- Symbole (aus Vantage, icons.ts) ----------

const g = (a: string, b: string) => `<defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${a}"/><stop offset="1" stop-color="${b}"/></linearGradient></defs>`;
const HEAD = "M12 21.4c-3-1.6-5.3-4.2-5.5-7.6L2.6 9l4.5 1.1-1.6-6.6 4.4 4.1c.8-.6 1.4-.8 2.1-.8s1.3.2 2.1.8l4.4-4.1-1.6 6.6 4.5-1.1-3.9 4.8c-.2 3.4-2.5 6-5.5 7.6z";
const SVG: Record<string, string> = {
  gold: '<ellipse cx="9" cy="15.6" rx="6" ry="2.6" fill="#b8913f"/><ellipse cx="9" cy="14.2" rx="6" ry="2.6" fill="#e3bd6d"/><ellipse cx="15" cy="10.4" rx="6" ry="2.6" fill="#b8913f"/><ellipse cx="15" cy="9" rx="6" ry="2.6" fill="#ffd86b"/><ellipse cx="15" cy="9" rx="3" ry="1.1" fill="none" stroke="#b8913f" stroke-width="0.8"/>',
  kills: '<path d="M4.2 3.2l9.4 9.4-1.6 1.6-9.4-9.4zM19.8 3.2l-9.4 9.4 1.6 1.6 9.4-9.4z" fill="currentColor"/><path d="M7.4 15.4l1.6 1.6-3.4 3.4-1.6-1.6zM16.6 15.4l-1.6 1.6 3.4 3.4 1.6-1.6z" fill="currentColor" opacity="0.75"/><path d="M6 13.2l4.8 4.8M18 13.2l-4.8 4.8" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>',
  tower: '<path d="M5 21.6h14l-1.4-2.4H6.4z" fill="currentColor"/><path d="M7.8 19.2 9.4 10.8h5.2l1.6 8.4z" fill="currentColor"/><path d="M6.8 10.8h10.4l-1-2H7.8z" fill="currentColor"/><path d="M7.8 8.8 4.6 5.6l4.6 1.6zM16.2 8.8l3.2-3.2-4.6 1.6z" fill="currentColor" opacity="0.8"/><path d="M9.2 8.8 10.2 5 12 1.6 13.8 5l1 3.8z" fill="currentColor"/><path d="M12 4.2l1.3 2.2L12 8.2l-1.3-1.8z" fill="#fff" opacity="0.9"/>',
  inhib: '<path d="M4.4 21.6h15.2l-2-3.2H6.4z" fill="currentColor" opacity="0.8"/><circle cx="12" cy="10" r="7" fill="none" stroke="currentColor" stroke-width="2"/><path d="M12 3.6l3.6 6.4-3.6 6.4-3.6-6.4z" fill="currentColor"/><path d="M12 3.6l3.6 6.4H8.4z" fill="#fff" opacity="0.5"/>',
  baron: `${g("#c99bff", "#6a2fc4")}<path d="M12 22.2c-4-1.5-7-5-7-9.5 0-3.4 1.9-5.6 4-6.4L7.9 1.8l2.7 2.9L12 1.2l1.4 3.5 2.7-2.9-1.1 4.5c2.1.8 4 3 4 6.4 0 4.5-3 8-7 9.5z" fill="url(#g)"/><path d="M5.2 12.4 1.6 10l1.2 4.6zM18.8 12.4l3.6-2.4-1.2 4.6z" fill="#7d3fd6"/><path d="M7.4 10.8l3.6 1.6-.4 1-3.4-1.3zM16.6 10.8 13 12.4l.4 1 3.4-1.3z" fill="#ffe36b"/><path d="M8.6 16.2 10 19l.9-2.4 1.1 3 1.1-3 .9 2.4 1.4-2.8c-2.2.9-4.6.9-6.8 0z" fill="#0b0d12"/>`,
  infernal: `${g("#ffb347", "#e8401c")}<path d="M12 1.8c1.3 3.4 6.2 5.9 6.2 12a6.2 6.2 0 0 1-12.4 0c0-3.1 1.6-5.1 3.3-6.4 0 2.2.9 3.5 2 3.9-.6-3.5.1-6.6.9-9.5z" fill="url(#g)"/><path d="M12.3 11.6c1.1 1.6 2.7 2.7 2.7 4.7a3 3 0 0 1-6 0c0-1 .4-1.8 1.1-2.4.1.9.6 1.5 1.1 1.6-.3-1.5.2-2.7 1.1-3.9z" fill="#ffe2a3"/>`,
  ocean: `${g("#7fe0ff", "#1f7fd6")}<path d="M12 1.8c3 4.2 6.4 7.6 6.4 12.4a6.4 6.4 0 0 1-12.8 0c0-4.8 3.4-8.2 6.4-12.4z" fill="url(#g)"/><path d="M7.7 15.2c1.4-1.9 3.5-2 4.6-.3.9 1.4 2.6 1.4 3.7-.1" fill="none" stroke="#e8fbff" stroke-width="1.5" stroke-linecap="round"/>`,
  mountain: `${g("#d9b48a", "#8a5a34")}<path d="M1.8 20.2 8.9 6.6l3.5 5.6 2.6-3.4 7.2 11.4z" fill="url(#g)"/><path d="M8.9 6.6l2.1 3.4-1.3-.6-1.1 1.1-1.1-1.1-1.1.5z" fill="#fbeedd"/>`,
  cloud: '<g fill="none" stroke="#dbe9f5" stroke-width="2" stroke-linecap="round"><path d="M2.8 9.6h10.6a3 3 0 1 0-3-3"/><path d="M2.8 13.8h14.8a3 3 0 1 1-3 3"/><path d="M5.6 18h5.4" stroke-opacity="0.7"/></g>',
  hextech: '<path d="M12 2.4l8.3 4.8v9.6L12 21.6l-8.3-4.8V7.2z" fill="none" stroke="#3fe0e8" stroke-width="1.8" stroke-linejoin="round"/><path d="M12 7.6l3.8 2.2v4.4L12 16.4l-3.8-2.2V9.8z" fill="#3fe0e8"/>',
  chemtech: `${g("#d6ff7a", "#4f9e1f")}<path d="M9.2 2.4h5.6v1.8h-.9v4.6l4.6 7.9a2.7 2.7 0 0 1-2.3 4.1H7.8a2.7 2.7 0 0 1-2.3-4.1l4.6-7.9V4.2h-.9z" fill="#2c3a1c" stroke="#b8f060" stroke-width="1.1" stroke-linejoin="round"/><path d="M7.6 14.4h8.8l1.7 2.9a1.4 1.4 0 0 1-1.2 2.1H7.1a1.4 1.4 0 0 1-1.2-2.1z" fill="url(#g)"/>`,
  elder: `${g("#f3e1ff", "#9b6bd8")}<path d="${HEAD}" fill="url(#g)"/>`,
  dragon: `${g("#d4dbe3", "#7d8894")}<path d="${HEAD}" fill="url(#g)"/>`,
};
let uid = 0;
function ic(key: string, color = "") {
  const s = mk("span", "es-ic");
  if (color) s.style.color = color;
  const id = `esg${++uid}`;
  s.innerHTML = `<svg viewBox="0 0 24 24" aria-hidden="true">${(SVG[key] ?? SVG.dragon).split('id="g"').join(`id="${id}"`).split("url(#g)").join(`url(#${id})`)}</svg>`;
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
      v.append(ic("gold"), mk("span", "", k(t.gold)));
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
      tw.append(ic("tower", s === "b" ? "#cfe0ff" : "#fff1cc"), mk("span", "", String(t.towers ?? 0)));
      const gd = mk("span", "es-stat gd");
      gd.append(ic("gold"), mk("span", "", k(t.gold)));
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
    mid.append(mk("b", "", String(d.blue.kills ?? 0)), ic("kills", "#8fbcff"), ic("kills", "#e3bd6d"), mk("b", "", String(d.red.kills ?? 0)));
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
      x.append(ic(key, s === "b" ? "#cfe0ff" : "#fff1cc"), mk("span", "", String(n)));
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
