/**
 * Eigenes LoL-Spiel in der Notch — Daten kommen von Vantage (Activity `vantage:game` mit Feld `lol`, ingame.rs).
 * Statt nur der Spielzeit, was gleich wichtig wird:
 *
 * - Klein (Mitte): links das Wichtigste jetzt — Welle 10 s vorher (normal, Kanone, Super-Vasallen), Objective in der
 *   letzten Minute vor dem Spawn, „ist da“, gerade erledigt, sonst der nächste Countdown; tot: Respawn. Rechts das
 *   CS-Ziel: CS, Ring bis zum nächsten Meilenstein, Vorsprung/Rückstand zum Plan.
 * - Aufgeklappt: Kopf mit Champion, K/D/A, CS und Clip-Knopf; CS-Ziel als Leiste mit Meilensteinen alle 5 Minuten
 *   (Soll-Marke wandert live mit, jeder CS füllt nach); die nächsten Wellen; Objectives; Drachen beider Teams und
 *   fehlende Inhibitoren (Super-Vasallen).
 *
 * Alle Zeiten sind Spielzeit; zwischen zwei Meldungen zählt die Notch selbst weiter (`t` zur Wanduhr `at`).
 * Symbole: Originale aus dem Spiel (lolicons.ts).
 */

import "@fontsource/cinzel/700.css";
import "@fontsource/saira-semi-condensed/500.css";
import "@fontsource/saira-semi-condensed/600.css";
import "@fontsource/saira-semi-condensed/700.css";
import { LOL, LOL_MASK, type LolIcon } from "./lolicons";

type Lane = "top" | "mid" | "bot";
export type LolWave = { at: number; cannon: boolean; sup: Lane[]; esup: Lane[] };
export type LolObj = { key: "dragon" | "elder" | "grubs" | "herald" | "baron"; type?: string; at: number; until?: number; left?: number };
export type LolGame = {
  t: number; at: number; paused: boolean; sr: boolean; goal: number | null;
  me?: { champ: string; level: number; k: number; d: number; a: number; cs: number; dead: boolean; respawn: number; pos: string; blue: boolean };
  /** CS beim Erreichen jeder 5-Minuten-Marke (null = nicht miterlebt) */
  marks?: (number | null)[];
  waves?: LolWave[];
  inhibs?: { ours: boolean; lane: Lane; back: number }[];
  drakes?: { ally: string[]; enemy: string[] };
  objs?: LolObj[];
  recent?: { key: string; type?: string; t: number; ally: boolean | null }[];
};

function mk<K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
}

/** Original-Symbol; Masken (Baron, Herald, Inhibitor …) in `color` */
function ic(key: string, color = "", cls = "lg-ic") {
  const s = mk("span", cls);
  const k = (key in LOL ? key : "dragon") as LolIcon;
  if (LOL_MASK.has(k)) {
    s.classList.add("m");
    s.style.setProperty("--m", `url("${LOL[k]}")`);
    s.style.color = color || "#c9a2ff";
  } else s.style.backgroundImage = `url("${LOL[k]}")`;
  return s;
}

// ---------- Zeit ----------

/** Spielzeit jetzt (zählt zwischen zwei Meldungen weiter, höchstens 8 s ohne neue) */
export const lgNow = (d: LolGame) => (d.paused ? d.t : d.t + Math.min(8, Math.max(0, (Date.now() - d.at) / 1000)));
const mmss = (s: number) => {
  const x = Math.max(0, Math.ceil(s));
  return `${Math.floor(x / 60)}:${String(x % 60).padStart(2, "0")}`;
};
const LANE: Record<Lane, string> = { top: "Top", mid: "Mid", bot: "Bot" };
const lanes = (l: Lane[]) => l.map((x) => LANE[x]).join(" · ");
const ELEMENT: Record<string, string> = { infernal: "Inferno", ocean: "Ozean", mountain: "Berg", cloud: "Wolken", hextech: "Hextech", chemtech: "Chemtech" };
const OBJ_NAME: Record<string, string> = { dragon: "Drache", elder: "Elder", grubs: "Larven", herald: "Herold", baron: "Baron" };
const objName = (o: { key: string; type?: string }) => (o.key === "dragon" && o.type && ELEMENT[o.type] ? `${ELEMENT[o.type]}-Drache` : OBJ_NAME[o.key] ?? o.key);
const objIcon = (o: { key: string; type?: string }, color = "") =>
  o.key === "dragon" || o.key === "elder" ? ic(o.type || (o.key === "elder" ? "elder" : "dragon")) : ic(o.key, color || (o.key === "baron" ? "#b48cff" : "#c9a2ff"));
const ALLY = "#7fb0ff", ENEMY = "#ff6f72";

// ---------- Wellen ----------

type WaveKind = "melee" | "cannon" | "super" | "esuper";
const waveKind = (w: LolWave): WaveKind => (w.sup.length ? "super" : w.cannon ? "cannon" : w.esup.length ? "esuper" : "melee");
const WAVE_ICON: Record<WaveKind, LolIcon> = { melee: "melee", cannon: "cannon", super: "super", esuper: "superRed" };
function waveLabel(w: LolWave) {
  const k = waveKind(w);
  if (k !== "esuper" && w.esup.length) return `${k === "super" ? `Super ${lanes(w.sup)}` : k === "cannon" ? "Kanone" : "Welle"} · Gegner-Super ${lanes(w.esup)}`;
  return k === "super" ? `Super-Vasallen ${lanes(w.sup)}` : k === "cannon" ? "Kanonen-Welle" : k === "esuper" ? `Gegner-Super ${lanes(w.esup)}` : "Welle";
}
function portrait(k: WaveKind, cls = "lg-wv") {
  const i = mk("span", `${cls} ${k}`);
  i.style.backgroundImage = `url("${LOL[WAVE_ICON[k]]}")`;
  return i;
}

// ---------- Was gerade zählt ----------

type Pick = { tone: "dead" | "wave" | "cannon" | "super" | "esuper" | "done" | "lost" | "soon" | "up" | "next" | "clock"; icon: HTMLElement; label: string; time?: string };

/** das Wichtigste jetzt (für die kleine Notch) */
export function lgPick(d: LolGame, now = lgNow(d)): Pick {
  const me = d.me;
  if (me?.dead && me.respawn > 0) {
    const left = d.t + me.respawn - now;
    if (left > 0) return { tone: "dead", icon: mk("span", "lg-skull", "✕"), label: "Respawn", time: mmss(left) };
  }
  const w = (d.waves ?? []).find((x) => x.at - now > -1.5 && x.at - now <= 10);
  if (w) {
    const k = waveKind(w);
    return { tone: k === "melee" ? "wave" : k, icon: portrait(k), label: waveLabel(w), time: w.at - now <= 0.5 ? "jetzt" : mmss(w.at - now) };
  }
  const r = (d.recent ?? []).filter((x) => now - x.t < 10).slice(-1)[0];
  if (r) return { tone: r.ally === false ? "lost" : "done", icon: objIcon(r), label: r.ally === false ? `Gegner: ${objName(r)}` : `${objName(r)} geholt`, time: r.ally === false ? undefined : "✓" };
  const objs = (d.objs ?? []).filter((o) => !o.until || now < o.until);
  const soon = objs.filter((o) => o.at > now && o.at - now <= 60).sort((a, b) => a.at - b.at)[0];
  if (soon) return { tone: "soon", icon: objIcon(soon), label: objName(soon), time: mmss(soon.at - now) };
  const up = objs.find((o) => o.at <= now);
  if (up) return { tone: "up", icon: objIcon(up), label: `${objName(up)} ${up.key === "grubs" ? "sind" : "ist"} da`, time: up.left && up.left < 3 ? `${up.left} übrig` : undefined };
  const next = objs.filter((o) => o.at > now).sort((a, b) => a.at - b.at)[0];
  if (next) return { tone: "next", icon: objIcon(next), label: objName(next), time: mmss(next.at - now) };
  return { tone: "clock", icon: mk("span", "lg-dot"), label: "Spielzeit", time: mmss(now) };
}

// ---------- CS-Ziel ----------

const MS = 300;
export function lgGoal(d: LolGame, now = lgNow(d)) {
  const g = d.goal ?? 0;
  const cs = d.me?.cs ?? 0;
  const want = (t: number) => Math.ceil((g * t) / 60);
  const plan = (g * now) / 60;
  const nextAt = (Math.floor(now / MS) + 1) * MS;
  const need = Math.max(0, want(nextAt) - cs);
  const left = nextAt - now;
  return {
    goal: g, cs, plan, delta: Math.round(cs - plan),
    rate: now > 60 ? cs / (now / 60) : 0,
    nextAt, nextCs: want(nextAt), need, left,
    /** nötiges Tempo bis zum nächsten Meilenstein */
    pace: left > 5 ? need / (left / 60) : 0,
    want,
  };
}
const num = (x: number, digits = 1) => x.toFixed(digits).replace(".", ",");
const signed = (n: number) => (n > 0 ? `+${n}` : n < 0 ? `−${-n}` : "±0");

function ring(frac: number) {
  const r = 7, c = 2 * Math.PI * r;
  const s = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  s.setAttribute("viewBox", "0 0 18 18");
  s.setAttribute("class", "lg-ring");
  s.innerHTML = `<circle cx="9" cy="9" r="${r}" class="t"/><circle cx="9" cy="9" r="${r}" class="p" stroke-dasharray="${c.toFixed(2)}" stroke-dashoffset="${(c * (1 - Math.max(0, Math.min(1, frac)))).toFixed(2)}"/>`;
  return s;
}

/** Unterschrift fürs Neuzeichnen: Daten und die angezeigte Sekunde */
export function lgSig(d: LolGame, extra = "") {
  return JSON.stringify([d.me, d.goal, d.waves?.[0], d.objs, d.recent, d.inhibs, d.drakes, d.marks, d.sr, d.paused, Math.floor(lgNow(d)), extra]);
}

// ---------- Klein ----------

/** Mitte der kleinen Notch; `full` = die Notch gehört ihr allein */
export function lgMid(d: LolGame, full: boolean) {
  const now = lgNow(d);
  const p = lgPick(d, now);
  const box = mk("div", `lg-mid ${full ? "full" : "shared"}`);
  const chip = mk("span", `lg-chip ${p.tone}`);
  chip.dataset.key = "chip";
  chip.append(p.icon, mk("span", "lg-c-l", p.label));
  if (p.time) chip.append(mk("b", "lg-c-t", p.time));
  box.append(chip);
  if (d.goal && d.me) {
    const g = lgGoal(d, now);
    const w = mk("span", `lg-m-goal ${g.delta >= 0 ? "ahead" : "behind"}`);
    w.dataset.key = "goal";
    w.title = `Ziel ${num(g.goal)} CS/min`;
    w.append(ring(g.cs / Math.max(1, g.nextCs)), mk("b", "lg-m-cs", String(g.cs)));
    if (full) w.append(mk("span", "lg-m-d", signed(g.delta)));
    box.append(w);
  } else if (d.me && full) {
    box.append(mk("span", "lg-m-kda", `${d.me.k}/${d.me.d}/${d.me.a}`));
  }
  return box;
}

// ---------- Aufgeklappt ----------

function head(d: LolGame, icon: string | undefined, onClip: () => void) {
  const h = mk("div", "lg-head");
  const me = d.me;
  const av = mk("span", `lg-av${me?.dead ? " dead" : ""}`);
  if (icon && /^data:image\//.test(icon)) av.style.backgroundImage = `url("${icon}")`;
  const who = mk("span", "lg-who");
  who.append(mk("b", "", me?.champ ?? "Im Spiel"), mk("span", "", me ? `Level ${me.level}${me.dead ? " · tot" : ""}` : ""));
  const kda = mk("span", "lg-kda");
  if (me) kda.append(mk("b", "", `${me.k}/${me.d}/${me.a}`), mk("span", "", `${me.cs} CS`));
  const clock = mk("span", `lg-clock${d.paused ? " paused" : ""}`, d.paused ? "Pause" : mmss(lgNow(d)));
  const clip = mk("button", "lg-clip", "Clip");
  clip.title = "Die letzten Sekunden als Clip speichern (Alt+F10)";
  clip.onclick = (e) => { e.stopPropagation(); onClip(); };
  h.append(av, who, kda, mk("span", "lg-gap"), clock, clip);
  return h;
}

function goalBlock(d: LolGame, now: number) {
  const g = lgGoal(d, now);
  const box = mk("div", `lg-goal ${g.delta >= 0 ? "ahead" : "behind"}`);
  box.dataset.key = "goal";
  const top = mk("div", "lg-g-top");
  const t = mk("span", "lg-g-title");
  t.append(mk("b", "", "CS-Ziel"), mk("span", "", `${num(g.goal)} pro Minute`));
  const now2 = mk("span", "lg-g-now");
  now2.append(mk("b", "lg-g-rate", num(g.rate)), mk("span", "", "/min"), mk("span", "lg-g-delta", `${signed(g.delta)} CS`));
  top.append(t, now2);

  // Leiste von 0 bis zum übernächsten Meilenstein; Marken alle 5 Minuten
  const last = Math.floor(now / MS) + 2;
  const max = g.want(last * MS);
  const bar = mk("div", "lg-bar");
  const fill = mk("i", "lg-fill");
  fill.dataset.key = "fill";
  fill.style.width = `${Math.min(100, (g.cs / max) * 100).toFixed(2)}%`;
  const plan = mk("i", "lg-plan");
  plan.dataset.key = "plan";
  plan.style.left = `${Math.min(100, (g.plan / max) * 100).toFixed(2)}%`;
  bar.append(fill, plan);
  const ticks = mk("div", "lg-ticks");
  for (let k = 1; k <= last; k++) {
    const at = k * MS, want = g.want(at);
    const past = now >= at;
    const had = d.marks?.[k - 1];
    const state = past ? (had == null ? "past" : had >= want ? "hit" : "miss") : g.cs >= want ? "hit early" : at === g.nextAt ? "next" : "later";
    const tk = mk("span", `lg-tick ${state}`);
    tk.dataset.key = `m${k}`;
    tk.style.left = `${((want / max) * 100).toFixed(2)}%`;
    tk.append(mk("i"), mk("b", "", `${k * 5}′`), mk("span", "", String(want)));
    if (past && had != null) tk.title = `${k * 5}:00 – ${had} von ${want} CS`;
    ticks.append(tk);
  }
  const hint = mk("div", "lg-g-hint");
  hint.textContent = g.need === 0
    ? `Meilenstein ${mmss(g.nextAt)} (${g.nextCs} CS) schon geschafft – weiter so`
    : `Bis ${mmss(g.nextAt)}: noch ${g.need} CS in ${mmss(g.left)}${g.pace ? ` · ${num(g.pace)}/min nötig` : ""}`;
  box.append(top, bar, ticks, hint);
  return box;
}

function wavesRow(d: LolGame, now: number) {
  const box = mk("div", "lg-sec lg-waves");
  box.append(mk("span", "lg-h", "Wellen"));
  const list = mk("div", "lg-wlist");
  for (const w of (d.waves ?? []).filter((x) => x.at > now - 1.5).slice(0, 4)) {
    const k = waveKind(w);
    const it = mk("span", `lg-w ${k}${w.at - now <= 10 ? " soon" : ""}`);
    it.dataset.key = `w${w.at}`;
    it.title = waveLabel(w);
    it.append(portrait(k), mk("b", "", w.at - now <= 0.5 ? "jetzt" : mmss(w.at - now)), mk("span", "", k === "super" ? `Super ${lanes(w.sup)}` : k === "cannon" ? "Kanone" : k === "esuper" ? "Gegner-Super" : "normal"));
    if (k !== "esuper" && w.esup.length) it.append(portrait("esuper", "lg-wv mini"));
    list.append(it);
  }
  box.append(list);
  return box;
}

function objRow(d: LolGame, now: number) {
  const box = mk("div", "lg-sec lg-objs");
  box.append(mk("span", "lg-h", "Objectives"));
  const grid = mk("div", "lg-ogrid");
  const recent = d.recent ?? [];
  for (const o of d.objs ?? []) {
    if (o.until && now >= o.until) continue;
    const up = o.at <= now;
    const soon = !up && o.at - now <= 60;
    const it = mk("span", `lg-o${up ? " up" : soon ? " soon" : ""}`);
    it.dataset.key = o.key;
    const txt = mk("span", "lg-o-t");
    const state = up
      ? o.until ? `${o.key === "grubs" ? "sind" : "ist"} da · bis ${mmss(o.until)}${o.left && o.left < 3 ? ` · ${o.left} übrig` : ""}` : "ist da"
      : `in ${mmss(o.at - now)}`;
    txt.append(mk("b", "", objName(o)), mk("span", "", state));
    it.append(objIcon(o), txt);
    grid.append(it);
  }
  // gerade erledigt (auch, wenn das Objective nicht wiederkommt)
  for (const r of recent.filter((x) => now - x.t < 30)) {
    const it = mk("span", `lg-o ${r.ally === false ? "lost" : "done"}`);
    it.dataset.key = `r${r.key}${r.t}`;
    const txt = mk("span", "lg-o-t");
    txt.append(mk("b", "", objName(r)), mk("span", "", r.ally === false ? "Gegner hat ihn" : r.ally ? "geholt ✓" : "erledigt"));
    it.append(objIcon(r), txt);
    grid.append(it);
  }
  box.append(grid);
  return box;
}

function teamsRow(d: LolGame, now: number) {
  const box = mk("div", "lg-teams");
  const side = (list: string[], ally: boolean) => {
    const w = mk("span", `lg-dr ${ally ? "a" : "e"}`);
    w.append(mk("span", "lg-dr-n", ally ? "Wir" : "Gegner"));
    for (const t of list) w.append(ic(t));
    if (list.length >= 4) w.append(mk("b", "lg-soul", "Seele"));
    if (!list.length) w.append(mk("span", "lg-dr-0", "keine Drachen"));
    return w;
  };
  box.append(side(d.drakes?.ally ?? [], true), side(d.drakes?.enemy ?? [], false));
  for (const i of d.inhibs ?? []) {
    const w = mk("span", `lg-inh ${i.ours ? "ours" : "theirs"}`);
    w.append(ic("inhib", i.ours ? ENEMY : ALLY), mk("span", "", `${i.ours ? "Gegner-Super" : "Super"} ${LANE[i.lane]} · noch ${mmss(i.back - now)}`));
    box.append(w);
  }
  return box;
}

/** Karte in der aufgeklappten Notch */
export function lgCard(d: LolGame, icon: string | undefined, onClip: () => void) {
  const now = lgNow(d);
  const card = mk("div", "lg");
  card.append(head(d, icon, onClip));
  if (d.goal && d.me) card.append(goalBlock(d, now));
  if (d.sr) card.append(wavesRow(d, now), objRow(d, now), teamsRow(d, now));
  return card;
}

/** CS dazu: Zahl hüpft, „+1“ steigt auf; Meilenstein erreicht: Marke leuchtet (WAAPI — morph würde Klassen entfernen) */
let lastCs = -1;
let lastHit = -1;
export function lgAnimate(root: ParentNode, d: LolGame) {
  const cs = d.me?.cs ?? -1;
  const g = lgGoal(d);
  const hit = cs >= g.nextCs ? g.nextAt : g.nextAt - MS;
  if (lastCs >= 0 && cs > lastCs) {
    for (const n of root.querySelectorAll<HTMLElement>(".lg-m-cs, .lg-kda b + span, .lg-g-rate")) {
      n.animate([{ transform: "scale(1)" }, { transform: "scale(1.28)", color: "#ffe9a8" }, { transform: "scale(1)" }], { duration: 420, easing: "cubic-bezier(.2,.8,.2,1)" });
    }
    const bar = root.querySelector<HTMLElement>(".lg-bar");
    if (bar) {
      const f = document.createElement("span");
      f.className = "lg-plus";
      f.textContent = `+${cs - lastCs}`;
      const fill = bar.querySelector<HTMLElement>(".lg-fill");
      f.style.left = fill?.style.width ?? "0";
      bar.append(f);
      f.animate([{ opacity: 0, transform: "translate(-50%, 4px)" }, { opacity: 1, transform: "translate(-50%, -10px)", offset: 0.3 }, { opacity: 0, transform: "translate(-50%, -20px)" }], { duration: 900, easing: "ease-out" }).onfinish = () => f.remove();
    }
  }
  if (lastHit >= 0 && hit > lastHit && cs >= g.want(hit) && d.goal) {
    for (const n of root.querySelectorAll<HTMLElement>(".lg-tick.hit i, .lg-m-goal .lg-ring")) {
      n.animate([{ transform: "scale(1)", filter: "brightness(1)" }, { transform: "scale(1.9)", filter: "brightness(1.8)" }, { transform: "scale(1)", filter: "brightness(1)" }], { duration: 900, easing: "cubic-bezier(.2,.8,.2,1)" });
    }
  }
  lastCs = cs;
  lastHit = hit;
}

/** für den Sprachassistenten */
export function lgVoice(d: LolGame) {
  const now = lgNow(d);
  const g = d.goal && d.me ? lgGoal(d, now) : null;
  const w = (d.waves ?? []).filter((x) => x.at > now).slice(0, 3);
  return {
    spielzeit: mmss(now),
    champion: d.me?.champ, kda: d.me ? `${d.me.k}/${d.me.d}/${d.me.a}` : null, cs: d.me?.cs,
    cs_ziel: g ? { ziel_pro_minute: g.goal, aktuell_pro_minute: Math.round(g.rate * 10) / 10, abweichung_cs: g.delta, naechster_meilenstein: `${mmss(g.nextAt)} mit ${g.nextCs} CS`, fehlende_cs: g.need } : null,
    naechste_wellen: w.map((x) => `${waveLabel(x)} in ${mmss(x.at - now)}`),
    objectives: (d.objs ?? []).filter((o) => !o.until || now < o.until).map((o) => `${objName(o)}: ${o.at <= now ? "ist da" : `in ${mmss(o.at - now)}`}`),
    drachen: d.drakes, inhibitoren_unten: (d.inhibs ?? []).map((i) => `${i.ours ? "unserer" : "gegnerischer"} ${LANE[i.lane]} bis ${mmss(i.back)}`),
  };
}
