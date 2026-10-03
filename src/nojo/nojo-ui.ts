/**
 * Nojo UI — Verhalten zur gemeinsamen Designsprache (nojo-ui.css).
 * Quelle: D:\Dev\nojo-design (dort aendern, dann tools\sync.ps1); in den Apps nur eine Kopie unter src/nojo/.
 *
 * - glassLight():       Licht auf Glasflaechen folgt der Maus
 * - segments():         die helle Glasperle gleitet unter das gewaehlte Segment
 * - windowControls():   Fensterknoepfe als Glaspille oben rechts (wie Folio)
 * - lightScroller():    Lichtleiste statt Bildlaufleiste — duenner Leuchtstab, zeigt den Ort, laesst sich ziehen
 *                       (die breite Sprungleiste gibt es nur in Folio)
 */

/** Feine Liniensymbole wie in Folio (24er Raster, Strich 1,6) */
export const svgIcon = (d: string) =>
  `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${d}</svg>`;

export const ICONS = {
  min: svgIcon('<path d="M6 12h12"/>'),
  max: svgIcon('<rect x="6.5" y="6.5" width="11" height="11" rx="2"/>'),
  close: svgIcon('<path d="M7 7l10 10"/><path d="M17 7L7 17"/>'),
  back: svgIcon('<path d="M14.5 5 8 12l6.5 7"/>'),
  prev: svgIcon('<path d="M14.5 6 8.5 12l6 6"/>'),
  next: svgIcon('<path d="M9.5 6l6 6-6 6"/>'),
  settings: svgIcon('<circle cx="12" cy="12" r="3"/><path d="M12 3.5v2.2M12 18.3v2.2M3.5 12h2.2M18.3 12h2.2M6 6l1.6 1.6M16.4 16.4 18 18M6 18l1.6-1.6M16.4 7.6 18 6"/>'),
  widget: svgIcon('<rect x="4" y="6" width="16" height="12" rx="4"/><path d="M8 12h5"/>'),
  sun: svgIcon('<circle cx="12" cy="12" r="3.6"/><path d="M12 3.5v2M12 18.5v2M3.5 12h2M18.5 12h2M6 6l1.4 1.4M16.6 16.6 18 18M6 18l1.4-1.4M16.6 7.4 18 6"/>'),
  moon: svgIcon('<path d="M19 14.5A7.5 7.5 0 0 1 9.5 5 7.5 7.5 0 1 0 19 14.5z"/>'),
  auto: svgIcon('<rect x="4" y="5" width="16" height="11" rx="2"/><path d="M9 20h6M12 16v4"/>'),
};

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
  wc.className = "n-wc n-glass";
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