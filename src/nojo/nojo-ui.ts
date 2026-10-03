/**
 * Nojo UI — Verhalten zur gemeinsamen Designsprache (nojo-ui.css).
 * Quelle: D:\Dev\nojo-design (dort aendern, dann tools\sync.ps1); in den Apps nur eine Kopie unter src/nojo/.
 *
 * - glassLight():       Licht auf Glasflaechen folgt der Maus
 * - segments():         die helle Glasperle gleitet unter das gewaehlte Segment
 * - windowControls():   Fensterknoepfe als Glaspille oben rechts (wie Folio)
 * - lightScroller():    Lichtleiste statt Bildlaufleiste — duenner Leuchtstab, zeigt den Ort, laesst sich ziehen
 *                       (die breite Sprungleiste gibt es nur in Folio)
 * - ICONS / icon():     Symbole aus der Bibliothek (nojo-icons.ts, generiert aus assets/icons.mjs)
 * - liquid():           echtes Liquid Glass fuer .n-liquid — Folios Optik (Brechung am Rand, klare Mitte,
 *                       Lichtkante von oben links) als SVG-Filter im backdrop-filter
 */

import { icon, type IconName } from "./nojo-icons";
export { icon, iconNode, iconUrl, NOJO_ICONS, SOURCE_COLORS, type IconName } from "./nojo-icons";

/** Feine Liniensymbole wie in Folio (24er Raster, Strich 1,6) — nur noch fuer Uebergaenge; neue Symbole in die Bibliothek */
export const svgIcon = (d: string) =>
  `<svg viewBox="0 0 24 24" class="n-i n-i-line" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${d}</svg>`;

/** Haeufige Symbole als fertiger SVG-Text (aus der Bibliothek) */
const pick = (...names: IconName[]) => Object.fromEntries(names.map((n) => [n, icon(n)])) as Record<(typeof names)[number], string>;
export const ICONS = pick("min", "max", "close", "back", "prev", "next", "settings", "widget", "sun", "moon", "auto", "fullscreen", "plus", "check", "search", "open");

const reduced = () => matchMedia("(prefers-reduced-motion: reduce)").matches;
const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

// ---------- Licht auf Glas ----------

let lightOn = false;
/** Einmal aufrufen: jede .n-glass-Flaeche bekommt ein Licht, das der Maus folgt. */
export function glassLight() {
  if (lightOn) return;
  lightOn = true;
  let lit: HTMLElement | null = null;
  document.addEventListener("pointermove", (e) => {
    const g = (e.target as Element | null)?.closest?.(".n-glass") as HTMLElement | null;
    if (lit && lit !== g) lit.classList.remove("lit");
    lit = g;
    if (!g || reduced()) return;
    const r = g.getBoundingClientRect();
    g.style.setProperty("--lx", `${e.clientX - r.left}px`);
    g.style.setProperty("--ly", `${e.clientY - r.top}px`);
    g.classList.add("lit");
  }, { passive: true });
  document.addEventListener("pointerleave", () => { lit?.classList.remove("lit"); lit = null; });
}

// ---------- Segmente ----------

/** Haelt in jedem .n-seg (oder `sel`) eine Glasperle unter dem gewaehlten Knopf (.active, aria-pressed, aria-selected). */
export function segments(root: ParentNode = document, sel = ".n-seg") {
  const seen = new WeakSet<HTMLElement>();
  const place = (seg: HTMLElement) => {
    let thumb = seg.querySelector<HTMLElement>(":scope > .n-seg-thumb");
    if (!thumb) {
      thumb = document.createElement("span");
      thumb.className = "n-seg-thumb";
      seg.prepend(thumb);
    }
    if (!seen.has(seg)) { seen.add(seg); ro.observe(seg); }
    // beim ersten Platzieren (oder aus dem Unsichtbaren) nicht hineingleiten
    const jump = !thumb.offsetWidth || thumb.style.width === "0px" || !thumb.style.width;
    const on = seg.querySelector<HTMLElement>(":scope > button.active, :scope > button[aria-pressed='true'], :scope > button[aria-selected='true']");
    if (!on) { thumb.style.width = "0"; return; }
    if (jump) thumb.style.transition = "none";
    // fluessig: die Perle wird beim Gleiten flacher und laenger und federt am Ziel zurueck
    else if (thumb.style.left && thumb.style.left !== `${on.offsetLeft}px` && !reduced()) {
      thumb.classList.remove("moving");
      void thumb.offsetWidth;
      thumb.classList.add("moving");
      clearTimeout(Number(thumb.dataset.t));
      thumb.dataset.t = String(window.setTimeout(() => thumb!.classList.remove("moving"), 260));
    }
    thumb.style.left = `${on.offsetLeft}px`;
    thumb.style.width = `${on.offsetWidth}px`;
    if (jump) { void thumb.offsetWidth; thumb.style.transition = ""; }
  };
  const all = () => (root as ParentNode).querySelectorAll<HTMLElement>(sel).forEach(place);
  const ro = new ResizeObserver(() => all());
  const mo = new MutationObserver((list) => {
    if (list.some((r) => !(r.target as Element).classList?.contains("n-glass") && !(r.target as Element).classList?.contains("n-seg-thumb"))) requestAnimationFrame(all);
  });
  mo.observe(root instanceof Document ? root.body : (root as Node), { subtree: true, childList: true, attributes: true, attributeFilter: ["class", "aria-pressed", "aria-selected", "hidden"] });
  ro.observe(root instanceof Document ? root.body : (root as Element));
  all();
  document.fonts?.ready.then(all);
  return () => { mo.disconnect(); ro.disconnect(); };
}

// ---------- Fensterknoepfe ----------

export type WindowActions = { minimize?: () => void; toggleMaximize?: () => void; close: () => void; closeTitle?: string };

/** Glaspille oben rechts mit Minimieren, Maximieren, Schliessen (fehlende Aktionen = kein Knopf). */
export function windowControls(a: WindowActions, host: HTMLElement = document.body) {
  const wc = document.createElement("div");
  wc.className = "n-wc n-glass n-liquid";
  const add = (icon: string, title: string, fn: () => void, cls = "") => {
    const b = document.createElement("button");
    b.className = `n-ico sm ${cls}`.trim();
    b.title = title;
    b.setAttribute("aria-label", title);
    b.innerHTML = icon;
    b.addEventListener("click", (e) => { e.stopPropagation(); fn(); });
    wc.append(b);
  };
  if (a.minimize) add(ICONS.min, "Minimieren", a.minimize);
  if (a.toggleMaximize) add(ICONS.max, "Maximieren", a.toggleMaximize);
  add(ICONS.close, a.closeTitle ?? "Schließen", a.close, "danger");
  host.append(wc);
  return wc;
}

// ---------- Lichtleiste ----------
// Folio hat die volle Leiste (wird beim Verweilen breit, Seitensprung). Die anderen Apps brauchen
// das nicht: hier ist sie nur ein duenner, leuchtender Stab, der zeigt, wo man ist, beim Scrollen
// kurz aufleuchtet und sich ziehen laesst.

export type LightScrollerOpts = {
  /** Abstand oben/unten (z. B. unter Fensterknoepfen) */
  insetTop?: number;
  insetBottom?: number;
  /** Abstand zum rechten Rand */
  edge?: number;
};

/** Ersetzt die Bildlaufleiste von `sc` (ein scrollendes Element oder document.scrollingElement). */
export function lightScroller(sc: HTMLElement, opts: LightScrollerOpts = {}) {
  const doc = sc === document.scrollingElement || sc === document.documentElement || sc === document.body;
  const target = doc ? (document.scrollingElement as HTMLElement) : sc;
  (doc ? document.documentElement : sc).classList.add("n-scroll");
  if (doc) document.body.classList.add("n-scroll");

  const bar = document.createElement("div");
  bar.className = "n-scroller";
  bar.setAttribute("aria-hidden", "true");
  // Seite oder Bereich: Apps koennen die Seitenleiste unter Dialogen ausblenden
  bar.dataset.scope = doc ? "page" : "box";
  document.body.append(bar);

  const insetTop = opts.insetTop ?? 14;
  const insetBottom = opts.insetBottom ?? 14;
  const edge = opts.edge ?? 5;
  let glowTimer = 0;

  const metrics = () => {
    const r = doc ? new DOMRect(0, 0, innerWidth, innerHeight) : sc.getBoundingClientRect();
    const sh = target.scrollHeight;
    const ch = doc ? innerHeight : sc.clientHeight;
    const st = target.scrollTop;
    const trackH = Math.max(40, r.height - insetTop - insetBottom);
    const ph = clamp((ch / Math.max(1, sh)) * trackH, 34, 110);
    return { r, sh, ch, st, max: Math.max(1, sh - ch), trackH, ph };
  };

  // erstes Platzieren ohne Gleiten (sonst faehrt die Leiste beim Oeffnen von links herein)
  let placed = false;
  const layout = () => {
    const m = metrics();
    const overflow = m.sh > m.ch + 4;
    bar.classList.toggle("show", overflow);
    if (!overflow) return;
    if (!placed) {
      placed = true;
      bar.style.transition = "none";
      requestAnimationFrame(() => requestAnimationFrame(() => { bar.style.transition = ""; }));
    }
    const top = m.r.top + insetTop + (m.st / m.max) * (m.trackH - m.ph);
    Object.assign(bar.style, { top: `${top}px`, height: `${m.ph}px`, left: `${m.r.right - 6 - edge}px` });
  };

  // ziehen
  bar.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    e.preventDefault();
    bar.classList.add("dragging");
    bar.setPointerCapture(e.pointerId);
    const y0 = e.clientY, s0 = metrics().st;
    const move = (ev: PointerEvent) => {
      const m = metrics();
      target.scrollTop = s0 + ((ev.clientY - y0) * m.max) / Math.max(1, m.trackH - m.ph);
    };
    const up = () => {
      bar.classList.remove("dragging");
      bar.removeEventListener("pointermove", move);
      bar.removeEventListener("pointerup", up);
      bar.removeEventListener("pointercancel", up);
    };
    bar.addEventListener("pointermove", move);
    bar.addEventListener("pointerup", up);
    bar.addEventListener("pointercancel", up);
  });

  // beim Scrollen kurz heller (Licht = gerade passiert etwas)
  const onScroll = () => {
    requestAnimationFrame(layout);
    bar.classList.add("moving");
    clearTimeout(glowTimer);
    glowTimer = window.setTimeout(() => bar.classList.remove("moving"), 700);
  };
  const onResize = () => layout();
  (doc ? window : sc).addEventListener("scroll", onScroll, { passive: true });
  window.addEventListener("resize", onResize);
  // Inhalt waechst oder schrumpft (Reiter wechseln, Gruppen aufklappen): neu vermessen
  let pending = 0;
  const later = () => { cancelAnimationFrame(pending); pending = requestAnimationFrame(layout); };
  const ro = new ResizeObserver(later);
  ro.observe(doc ? document.body : sc);
  for (const c of (doc ? document.body : sc).children) ro.observe(c);
  // eigene Aenderungen (Leiste, Licht auf Glas) nicht mitzaehlen, sonst misst sie sich endlos neu
  const mo = new MutationObserver((list) => {
    if (list.some((r) => r.target !== bar && !(r.type === "attributes" && (r.target as Element).classList?.contains("n-glass")))) later();
  });
  mo.observe(doc ? document.body : sc, { subtree: true, childList: true, attributes: true, attributeFilter: ["hidden", "open", "class"] });
  layout();
  return {
    layout,
    /** Leiste entfernen (Bereich wird geschlossen) */
    dispose: () => {
      clearTimeout(glowTimer);
      mo.disconnect();
      ro.disconnect();
      (doc ? window : sc).removeEventListener("scroll", onScroll);
      window.removeEventListener("resize", onResize);
      bar.remove();
    },
  };
}
// ---------- Liquid Glass ----------
// Folio bricht die PDF-Seiten mit WebGL (glass.ts). In den anderen Apps liegt unter dem Glas normales HTML; dasselbe
// optische Modell laeuft hier als SVG-Filter im backdrop-filter (Chromium: WebView2 und Electron koennen das):
// - Brechung nur am Rand: dort wird der Hintergrund von weiter innen geholt (wie eine dicke, runde Glaskante),
//   die Mitte bleibt klar. Staerke wie in Folio: Rand^2 * Brechung, Randbreite hoechstens 12 px.
// - Lichtkante knapp innerhalb der Kontur, am hellsten dort, wo die Kante zum Licht oben links zeigt.
// Pro Groesse ein Filter (Displacement-Map + Lichtkante als Bild), geteilt und gezaehlt.
//
// Benutzung: class="n-liquid" (klar, fuer Knoepfe und Pillen) oder class="n-liquid panel" (staerker
// weichgezeichnet, fuer Flaechen mit Text). Ohne Unterstuetzung bleibt es beim normalen Glas.

const LIGHT_DIR = (() => { const l = Math.hypot(0.45, 0.9); return [-0.45 / l, -0.9 / l] as const; })();
const smooth = (a: number, b: number, x: number) => { const t = clamp((x - a) / (b - a), 0, 1); return t * t * (3 - 2 * t); };

type LiquidFilter = { id: string; rim: string; refs: number };
const liquidFilters = new Map<string, LiquidFilter>();
let liquidDefs: SVGSVGElement | null = null;
let liquidSeq = 0;

/** Kann dieser Browser SVG-Filter als backdrop-filter? (Chromium ja, Safari und Firefox nicht) */
export const liquidSupported = () =>
  typeof CSS !== "undefined" && CSS.supports("backdrop-filter", "url(#n)") && /Chrome\/\d+/.test(navigator.userAgent);

function liquidMaps(w: number, h: number, r: number, refract: number, dpr: number) {
  const W = Math.max(1, Math.round(w * dpr)), H = Math.max(1, Math.round(h * dpr));
  const disp = new ImageData(W, H), spec = new ImageData(W, H);
  const hw = w / 2, hh = h / 2, rr = Math.min(r, hw, hh);
  const edgeW = Math.max(1, Math.min(12, Math.min(hw, hh) * 0.65));
  const S = refract * 2 + 2;
  const aa = 0.6 / dpr + 0.25;
  for (let j = 0; j < H; j++) {
    for (let i = 0; i < W; i++) {
      const x = (i + 0.5) / dpr - hw, y = (j + 0.5) / dpr - hh;
      const qx = Math.abs(x) - hw + rr, qy = Math.abs(y) - hh + rr;
      const d = Math.min(Math.max(qx, qy), 0) + Math.hypot(Math.max(qx, 0), Math.max(qy, 0)) - rr;
      let nx: number, ny: number;
      if (qx > 0 && qy > 0) { const l = Math.hypot(qx, qy); nx = qx / l; ny = qy / l; }
      else if (qx > qy) { nx = 1; ny = 0; } else { nx = 0; ny = 1; }
      if (x < 0) nx = -nx;
      if (y < 0) ny = -ny;
      const depth = Math.max(-d, 0);
      const edge = 1 - smooth(0, edgeW, depth);
      const off = edge * edge * refract;
      const k = (j * W + i) * 4;
      // nach innen greifen: an der Kante liegt, was eigentlich weiter innen ist (Lupe am Rand)
      disp.data[k] = clamp(Math.round(128 - (nx * off / S) * 255), 0, 255);
      disp.data[k + 1] = clamp(Math.round(128 - (ny * off / S) * 255), 0, 255);
      disp.data[k + 2] = 128;
      disp.data[k + 3] = 255;
      const facing = Math.max(nx * LIGHT_DIR[0] + ny * LIGHT_DIR[1], 0) ** 3;
      const rim = Math.exp(-(((depth - 0.7) / 0.65) ** 2));
      const cover = 1 - smooth(-aa, aa, d);
      const a = (rim * (0.12 + 0.36 * facing) + 0.03 * edge * facing) * cover;
      spec.data[k] = spec.data[k + 1] = spec.data[k + 2] = 255;
      spec.data[k + 3] = clamp(Math.round(a * 255), 0, 255);
    }
  }
  const url = (img: ImageData) => {
    const c = document.createElement("canvas");
    c.width = W;
    c.height = H;
    c.getContext("2d")!.putImageData(img, 0, 0);
    return c.toDataURL("image/png");
  };
  return { disp: url(disp), rim: url(spec), scale: S };
}

function liquidFilter(w: number, h: number, r: number, refract: number) {
  const dpr = Math.min(3, Math.max(1, devicePixelRatio || 1));
  const key = `${w}x${h}r${r}f${refract}d${dpr}`;
  let f = liquidFilters.get(key);
  if (f) { f.refs++; return { key, f }; }
  if (!liquidDefs) {
    liquidDefs = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    liquidDefs.setAttribute("aria-hidden", "true");
    liquidDefs.style.cssText = "position:absolute;width:0;height:0;overflow:hidden;pointer-events:none";
    document.body.append(liquidDefs);
  }
  const m = liquidMaps(w, h, r, refract, dpr);
  const id = `n-lq-${++liquidSeq}`;
  liquidDefs.insertAdjacentHTML("beforeend",
    `<filter id="${id}" x="0" y="0" width="${w}" height="${h}" filterUnits="userSpaceOnUse" primitiveUnits="userSpaceOnUse" color-interpolation-filters="sRGB">` +
    `<feImage href="${m.disp}" x="0" y="0" width="${w}" height="${h}" preserveAspectRatio="none" result="m"/>` +
    `<feDisplacementMap in="SourceGraphic" in2="m" scale="${m.scale}" xChannelSelector="R" yChannelSelector="G"/></filter>`);
  f = { id, rim: m.rim, refs: 1 };
  liquidFilters.set(key, f);
  return { key, f };
}

function liquidRelease(key: string | undefined) {
  if (!key) return;
  const f = liquidFilters.get(key);
  if (!f || --f.refs > 0) return;
  liquidFilters.delete(key);
  liquidDefs?.querySelector(`#${f.id}`)?.remove();
}

let liquidOn = false;
/**
 * Einmal aufrufen: jedes .n-liquid (auch spaeter eingefuegte) bekommt Brechung und Lichtkante passend zu seiner
 * Groesse und Rundung. Aendert sich die Groesse laufend (Aufklappen), bleibt kurz der alte Filter stehen.
 */
export function liquid() {
  if (liquidOn || !liquidSupported()) return;
  liquidOn = true;
  const keys = new Map<HTMLElement, string>();
  const timers = new WeakMap<HTMLElement, number>();
  const drop = (el: HTMLElement) => { liquidRelease(keys.get(el)); keys.delete(el); ro.unobserve(el); };
  const apply = (el: HTMLElement) => {
    if (!el.isConnected || !el.classList.contains("n-liquid")) { drop(el); return; }
    const w = Math.round(el.offsetWidth), h = Math.round(el.offsetHeight);
    if (w < 4 || h < 4) return;
    const rt = getComputedStyle(el).borderTopLeftRadius.split(" ")[0];
    const r = Math.round(rt.endsWith("%") ? (parseFloat(rt) / 100) * Math.min(w, h) : parseFloat(rt) || 0);
    const panel = el.classList.contains("panel");
    const refract = Number(el.dataset.refract) || (panel ? 7 : 4.5);
    const before = keys.get(el);
    const { key, f } = liquidFilter(w, h, Math.min(r, Math.floor(Math.min(w, h) / 2)), refract);
    if (before === key) { f.refs--; return; }
    keys.set(el, key);
    el.style.setProperty("--n-lq", `url(#${f.id})`);
    el.style.setProperty("--n-lq-rim", `url("${f.rim}")`);
    el.dataset.lq = "";
    liquidRelease(before);
  };
  const later = (el: HTMLElement, wait = 90) => {
    clearTimeout(timers.get(el));
    timers.set(el, window.setTimeout(() => apply(el), keys.has(el) ? wait : 0));
  };
  const ro = new ResizeObserver((list) => list.forEach((e) => later(e.target as HTMLElement)));
  const scan = () => {
    document.querySelectorAll<HTMLElement>(".n-liquid").forEach((el) => {
      if (keys.has(el) || timers.has(el)) return;
      ro.observe(el);
      later(el, 0);
    });
    // entfernte Flaechen geben ihren Filter frei
    for (const el of [...keys.keys()]) if (!el.isConnected || !el.classList.contains("n-liquid")) drop(el);
  };
  let pending = 0;
  new MutationObserver(() => { cancelAnimationFrame(pending); pending = requestAnimationFrame(scan); })
    .observe(document.body, { subtree: true, childList: true, attributes: true, attributeFilter: ["class"] });
  scan();
}
