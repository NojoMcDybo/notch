/**
 * Verlaufs-Activities (Blutzucker aus Haze): Wert + Trendpfeil + Aenderung, Graph mit Zielbereich,
 * Zeitbereiche 3/6/12/24 Std., Punkt antippen = Zeit und Wert, Doppelklick = App nach vorn.
 *
 * Grundregel: alte Werte duerfen nie wie aktuelle aussehen. Ab STALE_MS ohne neuen Messwert wird
 * alles grau, Pfeil und Aenderung verschwinden, dafuer steht "vor X Min" da. Laeuft die ttl ab,
 * meldet Rust `expired` und hier steht "keine Daten" — der Eintrag bleibt aber stehen.
 * Alarme entscheidet Haze (Feld `alert`), die Notch zeigt nur an.
 */

export type ChartData = { low: number; high: number; points: [number, number][]; ranges?: number[]; range?: number };
export type BgActivity = {
  id: string; app: string; title: string; value?: string; unit?: string; trend?: string; delta?: number;
  chart?: ChartData; expired?: boolean; updated: number;
};

export const STALE_MS = 12 * 60_000;
const ARROWS: Record<string, string> = { up2: "⇈", up: "↑", up45: "↗", flat: "→", down45: "↘", down: "↓", down2: "⇊" };
const TREND_TEXT: Record<string, string> = {
  up2: "steigt sehr schnell", up: "steigt schnell", up45: "steigt", flat: "stabil",
  down45: "fällt", down: "fällt schnell", down2: "fällt sehr schnell",
};
const C = { ok: "#f5f5f7", high: "#EDBC56", low: "#FF7971", stale: "#8E9A9B" };

export const isBg = (a: { chart?: unknown }) => !!a.chart;

/** Zeitpunkt des letzten Messwerts (nicht der letzten Sendung — Haze frischt alle 60 s auf) */
function lastAt(a: BgActivity) {
  const p = a.chart?.points;
  return p && p.length ? p[p.length - 1][0] : a.updated;
}

export function bgStatus(a: BgActivity, now = Date.now()) {
  const age = Math.max(0, now - lastAt(a));
  const expired = !!a.expired;
  const stale = expired || age > STALE_MS;
  const v = Number(a.value);
  const lo = a.chart?.low ?? 70, hi = a.chart?.high ?? 180;
  const color = stale ? C.stale : v < lo ? C.low : v > hi ? C.high : C.ok;
  return { age, stale, expired, color, mins: Math.round(age / 60_000) };
}

const fmtDelta = (d?: number) => (d == null || !isFinite(d) ? "" : d > 0 ? `+${Math.round(d)}` : d < 0 ? `−${Math.abs(Math.round(d))}` : "±0");
const agoText = (mins: number) => (mins < 1 ? "gerade eben" : mins < 60 ? `vor ${mins} Min` : `vor ${Math.floor(mins / 60)} Std ${mins % 60} Min`);
const hhmm = (t: number) => new Date(t).toLocaleTimeString("de-DE", { hour: "2-digit", minute: "2-digit" });

function mk<K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  return e;
}
const SVG = "http://www.w3.org/2000/svg";
function sv(tag: string, attrs: Record<string, string | number>) {
  const e = document.createElementNS(SVG, tag);
  for (const k in attrs) e.setAttribute(k, String(attrs[k]));
  return e;
}

/** Signatur fuer den Vergleich "muss die Kompaktanzeige neu?" (Minuten zaehlen mit, sobald veraltet) */
export function bgSig(a: BgActivity) {
  const s = bgStatus(a);
  return [a.value, a.trend, a.delta, s.stale, s.expired, s.stale ? s.mins : -1, s.color].join("|");
}

/** Kompakt (zu): nur Wert + Pfeil + Aenderung. Veraltet: grau + "vor X Min". Abgelaufen: "keine Daten". */
export function fillCompact(trail: HTMLElement, a: BgActivity, o: { delta?: boolean } = {}) {
  const s = bgStatus(a);
  trail.style.setProperty("--accent", s.color);
  trail.classList.add("bg");
  if (s.expired) {
    trail.append(mk("span", "bg-v", a.value ?? "—"), mk("small", "", "keine Daten"));
    return;
  }
  trail.append(mk("span", "bg-v", a.value ?? "—"));
  if (s.stale) { trail.append(mk("small", "", agoText(s.mins))); return; }
  if (a.trend && ARROWS[a.trend]) trail.append(mk("b", "arrow", ARROWS[a.trend]));
  const d = o.delta === false ? "" : fmtDelta(a.delta);
  if (d) trail.append(mk("small", "bg-d", d));
}

/** Messwert ausserhalb des Zielbereichs (und aktuell, nicht veraltet)? */
export function bgOutOfRange(a: BgActivity) {
  const s = bgStatus(a);
  if (s.stale) return false;
  const v = Number(a.value);
  const lo = a.chart?.low ?? 70, hi = a.chart?.high ?? 180;
  return Number.isFinite(v) && (v < lo || v > hi);
}

// ---------- Aufgeklappt: Kopf + Graph ----------

const ui = new Map<string, { range?: number; sel?: number }>();
let pendingClick = 0;

export type BgOpts = { width: number; onDouble: () => void; onChange: () => void };

/** Baut die Karte in `row` (nur wenn sich etwas Sichtbares geaendert hat). Das ×-Element bleibt Sache von main. */
export function fillCard(row: HTMLElement, a: BgActivity, o: BgOpts) {
  const st = ui.get(a.id) ?? {};
  ui.set(a.id, st);
  const chart = a.chart!;
  const ranges = chart.ranges?.length ? chart.ranges : [3, 6, 12, 24];
  const range = st.range && ranges.includes(st.range) ? st.range : chart.range ?? ranges[0];
  const s = bgStatus(a);
  const pts = chart.points;
  const sig = [o.width, range, st.sel, pts.length, pts[pts.length - 1]?.[0], a.value, a.trend, a.delta, s.stale, s.expired, s.mins, a.title].join("|");
  if (row.dataset.bgsig === sig) return false;
  row.dataset.bgsig = sig;
  row.style.setProperty("--bg", s.color);

  // Kopf: links Titel + Wert/Pfeil/Aenderung, rechts Zeitbereich-Pillen
  const head = mk("div", "bg-head");
  const left = mk("div", "bg-left");
  const line = mk("div", "bg-line");
  line.append(mk("span", "bg-val", s.expired ? "—" : a.value ?? "—"));
  if (!s.stale && a.trend && ARROWS[a.trend]) {
    const ar = mk("span", "bg-arrow", ARROWS[a.trend]);
    ar.title = TREND_TEXT[a.trend];
    line.append(ar);
  }
  if (!s.stale && fmtDelta(a.delta)) line.append(mk("span", "bg-delta", fmtDelta(a.delta)));
  const meta = mk("div", "bg-meta");
  meta.textContent = s.expired
    ? `keine Daten · letzter Wert ${agoText(s.mins)}`
    : `${a.unit ?? "mg/dL"} · ${agoText(s.mins)}${s.stale ? " · veraltet" : ""}`;
  left.append(mk("div", "bg-title", a.title), line, meta);
  const pills = mk("div", "bg-pills");
  for (const r of ranges) {
    const b = mk("button", "bg-pill" + (r === range ? " on" : ""), `${r} Std.`);
    b.addEventListener("click", (e) => { e.stopPropagation(); st.range = r; st.sel = undefined; o.onChange(); });
    pills.append(b);
  }
  head.append(left, pills);

  const graph = drawGraph(a, range, st, o, s.stale);
  row.replaceChildren(head, graph);
  return true;
}

function drawGraph(a: BgActivity, range: number, st: { sel?: number }, o: BgOpts, stale: boolean) {
  const chart = a.chart!;
  const W = o.width, H = 132;
  const L = 4, R = W - 34, T = 10, B = H - 20;
  const now = Date.now();
  const t1 = Math.max(now, lastAt(a)), t0 = t1 - range * 3_600_000;
  const pts = chart.points.filter((p) => p[0] >= t0 && p[0] <= t1);
  const vals = pts.map((p) => p[1]);
  const yMin = Math.min(40, ...vals.map((v) => v - 8));
  const yMax = Math.max(260, ...vals.map((v) => v + 12));
  const x = (t: number) => L + ((t - t0) / (t1 - t0)) * (R - L);
  const y = (v: number) => B - ((v - yMin) / (yMax - yMin)) * (B - T);

  const svg = sv("svg", { class: "bg-graph" + (stale ? " stale" : ""), viewBox: `0 0 ${W} ${H}`, width: W, height: H });
  // Zeitmarken: 3 runde Uhrzeiten (3 h: stuendlich, 6 h: 2 h, 12 h: 4 h, 24 h: 8 h)
  const step = (range / 3) * 3_600_000;
  const off = new Date().getTimezoneOffset() * 60_000;
  let tick = Math.floor((t1 - off) / step) * step + off;
  const ticks: number[] = [];
  while (ticks.length < 3 && tick > t0 + step * 0.15) { ticks.unshift(tick); tick -= step; }
  for (const t of ticks) {
    const tx = x(t);
    svg.append(sv("line", { class: "bg-vgrid", x1: tx, x2: tx, y1: T, y2: B }));
    const lab = sv("text", { class: "bg-tlab", x: tx, y: H - 5, "text-anchor": "middle" });
    lab.textContent = hhmm(t);
    svg.append(lab);
  }
  // Grenzlinien mit Beschriftung rechts
  for (const [v, cls] of [[chart.high, "hi"], [chart.low, "lo"]] as const) {
    const ly = y(v);
    svg.append(sv("line", { class: `bg-lim ${cls}`, x1: L, x2: R, y1: ly, y2: ly }));
    const lab = sv("text", { class: `bg-llab ${cls}`, x: R + 6, y: ly + 3.5 });
    lab.textContent = String(v);
    svg.append(lab);
  }
  // Punkte: weiss im Ziel, bernstein ueber high, rot unter low
  const r = range <= 3 ? 2.8 : range <= 6 ? 2.4 : range <= 12 ? 2 : 1.6;
  for (const [t, v] of pts) {
    const cls = v > chart.high ? "hi" : v < chart.low ? "lo" : "ok";
    svg.append(sv("circle", { class: `bg-pt ${cls}`, cx: x(t).toFixed(1), cy: y(v).toFixed(1), r }));
  }
  // ausgewaehlter Punkt: Ring + Zeit und Wert oben
  const sel = st.sel != null ? pts.find((p) => p[0] === st.sel) : undefined;
  if (sel) {
    const sx = x(sel[0]), sy = y(sel[1]);
    svg.append(sv("line", { class: "bg-selline", x1: sx, x2: sx, y1: T, y2: B }));
    svg.append(sv("circle", { class: "bg-sel", cx: sx, cy: sy, r: r + 3.5 }));
    const label = `${hhmm(sel[0])} · ${Math.round(sel[1])} ${a.unit ?? "mg/dL"}`;
    const w = label.length * 6.1 + 14;
    const bx = Math.min(Math.max(L, sx - w / 2), R - w);
    const by = sy - 30 < T ? sy + 10 : sy - 30;
    svg.append(sv("rect", { class: "bg-tip", x: bx, y: by, width: w, height: 20, rx: 6 }));
    const tt = sv("text", { class: "bg-tiptext", x: bx + w / 2, y: by + 14, "text-anchor": "middle" });
    tt.textContent = label;
    svg.append(tt);
  }
  if (!pts.length) {
    const t = sv("text", { class: "bg-empty", x: (L + R) / 2, y: (T + B) / 2, "text-anchor": "middle" });
    t.textContent = "keine Werte in diesem Zeitraum";
    svg.append(t);
  }

  // Klick: naechster Punkt (max. 14 px daneben) -> Zeit und Wert; daneben -> Auswahl weg.
  // Doppelklick selbst erkennen: die Auswahl baut den Graphen neu, ein natives dblclick
  // ginge dabei verloren. Deshalb wartet die Auswahl kurz, ob ein zweiter Klick kommt.
  svg.addEventListener("click", (e) => {
    e.stopPropagation();
    if (pendingClick) {
      clearTimeout(pendingClick);
      pendingClick = 0;
      o.onDouble();
      return;
    }
    pendingClick = window.setTimeout(() => { pendingClick = 0; pick(e); }, 260);
  });
  const pick = (e: MouseEvent) => {
    const rect = svg.getBoundingClientRect();
    const mx = ((e.clientX - rect.left) / rect.width) * W, my = ((e.clientY - rect.top) / rect.height) * H;
    let best: [number, number] | null = null, bd = 14 * 14;
    for (const p of pts) {
      const dx = x(p[0]) - mx, dy = (y(p[1]) - my) * 0.35; // waagerecht zaehlt mehr
      const d = dx * dx + dy * dy;
      if (d < bd) { bd = d; best = p; }
    }
    const next = best ? best[0] : undefined;
    if (next !== st.sel) { st.sel = next; o.onChange(); }
  };
  svg.addEventListener("dblclick", (e) => e.stopPropagation());
  svg.addEventListener("contextmenu", (e) => e.stopPropagation());
  svg.setAttribute("title", "");
  const wrap = mk("div", "bg-graph-wrap");
  wrap.title = `Punkt antippen: Zeit und Wert · Doppelklick: ${a.app || "App"} öffnen`;
  wrap.append(svg);
  return wrap;
}
