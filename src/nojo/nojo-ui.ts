/**
 * Nojo UI — Verhalten zur gemeinsamen Designsprache (nojo-ui.css).
 * Quelle: D:\Dev\nojo-design (dort aendern, dann tools\sync.ps1); in den Apps nur eine Kopie unter src/nojo/.
 *
 * - glassLight():       Licht auf Glasflaechen folgt der Maus
 * - segments():         die helle Glasperle gleitet unter das gewaehlte Segment
 * - windowControls():   Fensterknoepfe als Glaspille oben rechts (wie Folio)
 * - lightScroller():    Lichtleiste statt Bildlaufleiste — zeigt den Ort, verweilt man, wird sie zur
 *                       Sprungleiste mit Abschnittsmarken (wie Folios Seitenleiste)
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

export type LightScrollerOpts = {
  /** Abschnitte (Element + Name) fuer Marken und Spruenge */
  sections?: () => { el: HTMLElement; label: string }[];
  /** Abstand oben/unten (z. B. unter Fensterknoepfen) */
  insetTop?: number;
  insetBottom?: number;
  /** Abstand zum rechten Rand */
  edge?: number;
};

/**
 * Ersetzt die Bildlaufleiste von `sc` (ein scrollendes Element oder document.scrollingElement).
 * Duenn zeigt sie, wo man ist, und laesst sich ziehen. Verweilt die Maus an ihr (oder ganz am rechten
 * Rand), wird sie breit: der sichtbare Teil als Licht, jeder Abschnitt als Marke. Zeigen = Name des
 * Abschnitts, Klick = dorthin, Mausrad = Abschnitt fuer Abschnitt.
 */
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
  const view = document.createElement("div");
  view.className = "n-scroller-view";
  bar.append(view);
  const tip = document.createElement("div");
  tip.className = "n-scroller-tip n-glass";
  document.body.append(bar, tip);

  let wide = false;
  let dragging = false;
  let openTimer = 0;
  let closeTimer = 0;
  let marks: { el: HTMLElement; top: number; label: string }[] = [];
  const insetTop = opts.insetTop ?? 14;
  const insetBottom = opts.insetBottom ?? 14;
  const edge = opts.edge ?? 5;

  const metrics = () => {
    const r = doc ? new DOMRect(0, 0, innerWidth, innerHeight) : sc.getBoundingClientRect();
    const sh = target.scrollHeight;
    const ch = doc ? innerHeight : sc.clientHeight;
    const st = target.scrollTop;
    return { r, sh, ch, st, max: Math.max(1, sh - ch) };
  };

  // Abschnitte: Position im gesamten Inhalt
  const readSections = () => {
    const m = metrics();
    const base = doc ? 0 : m.r.top;
    marks = (opts.sections?.() ?? [])
      .filter((s) => s.el.isConnected && s.el.offsetParent !== null)
      .map((s) => ({ el: s.el, label: s.label, top: s.el.getBoundingClientRect().top - base + m.st }))
      .sort((a, b) => a.top - b.top);
    bar.querySelectorAll(".n-scroller-mark").forEach((x) => x.remove());
    for (let i = 0; i < marks.length; i++) {
      const d = document.createElement("i");
      d.className = "n-scroller-mark";
      bar.append(d);
    }
  };

  // erstes Platzieren ohne Gleiten (sonst faehrt die Leiste beim Oeffnen von links herein)
  let placed = false;
  const layout = () => {
    const m = metrics();
    const overflow = m.sh > m.ch + 4;
    bar.classList.toggle("show", overflow);
    if (!overflow) { setWide(false); return; }
    if (!placed) {
      placed = true;
      bar.style.transition = "none";
      requestAnimationFrame(() => requestAnimationFrame(() => { bar.style.transition = ""; }));
    }
    const trackTop = m.r.top + insetTop;
    const trackH = Math.max(40, m.r.height - insetTop - insetBottom);
    if (wide) {
      const h = Math.min(trackH, Math.max(170, m.r.height * 0.72));
      const top = m.r.top + insetTop + (trackH - h) / 2;
      const w = 24;
      Object.assign(bar.style, { top: `${top}px`, height: `${h}px`, width: `${w}px`, left: `${m.r.right - w - edge - 3}px` });
      const vh = Math.max(12, (m.ch / m.sh) * (h - 6));
      view.style.top = `${3 + (m.st / m.sh) * (h - 6)}px`;
      view.style.height = `${vh}px`;
      bar.querySelectorAll<HTMLElement>(".n-scroller-mark").forEach((d, i) => {
        d.style.top = `${3 + (marks[i].top / m.sh) * (h - 6)}px`;
      });
    } else {
      const ph = clamp((m.ch / m.sh) * trackH, 34, 110);
      const frac = m.st / m.max;
      Object.assign(bar.style, { top: `${trackTop + frac * (trackH - ph)}px`, height: `${ph}px`, width: "6px", left: `${m.r.right - 6 - edge}px` });
    }
  };

  const setWide = (on: boolean) => {
    if (wide === on) return;
    wide = on;
    if (on) readSections();
    bar.classList.toggle("wide", on);
    if (!on) tip.classList.remove("show");
    layout();
  };

  // wo im Inhalt die Maus auf der breiten Leiste zeigt, und welcher Abschnitt dort liegt
  const pick = (clientY: number) => {
    const m = metrics();
    const r = bar.getBoundingClientRect();
    const pos = clamp((clientY - r.top - 3) / (r.height - 6), 0, 1) * m.sh;
    let idx = -1;
    for (let i = 0; i < marks.length; i++) if (marks[i].top <= pos + 1) idx = i;
    return { pos, idx, m };
  };

  const go = (top: number) => target.scrollTo({ top: clamp(top, 0, metrics().max), behavior: reduced() ? "auto" : "smooth" });

  bar.addEventListener("pointerenter", () => {
    clearTimeout(closeTimer);
    if (!wide && !dragging) openTimer = window.setTimeout(() => setWide(true), 420);
  });
  bar.addEventListener("pointerleave", () => {
    clearTimeout(openTimer);
    if (!dragging) closeTimer = window.setTimeout(() => setWide(false), 450);
    tip.classList.remove("show");
    bar.querySelectorAll(".n-scroller-mark.near").forEach((x) => x.classList.remove("near"));
  });
  bar.addEventListener("pointermove", (e) => {
    if (!wide) return;
    const { idx, pos, m } = pick(e.clientY);
    const label = idx >= 0 ? marks[idx].label : marks.length ? marks[0].label : `${Math.round((pos / m.sh) * 100)} %`;
    tip.textContent = label;
    tip.style.left = `${bar.getBoundingClientRect().left}px`;
    tip.style.top = `${e.clientY}px`;
    tip.classList.add("show");
    bar.querySelectorAll<HTMLElement>(".n-scroller-mark").forEach((d, i) => d.classList.toggle("near", i === idx));
  });
  bar.addEventListener("click", (e) => {
    if (!wide) return;
    const { idx, pos, m } = pick(e.clientY);
    go(idx >= 0 ? marks[idx].top - 12 : pos - m.ch / 2);
  });
  bar.addEventListener("wheel", (e) => {
    if (!wide || !marks.length) return;
    e.preventDefault();
    const st = metrics().st + 14;
    const next = e.deltaY > 0 ? marks.find((x) => x.top > st + 2) : [...marks].reverse().find((x) => x.top < st - 16);
    go(next ? next.top - 12 : e.deltaY > 0 ? metrics().max : 0);
  }, { passive: false });

  // duenn: ziehen
  bar.addEventListener("pointerdown", (e) => {
    if (wide || e.button !== 0) return;
    e.preventDefault();
    dragging = true;
    bar.classList.add("dragging");
    bar.setPointerCapture(e.pointerId);
    const y0 = e.clientY, s0 = metrics().st;
    const move = (ev: PointerEvent) => {
      const m = metrics();
      const trackH = m.r.height - insetTop - insetBottom;
      const ph = clamp((m.ch / m.sh) * trackH, 34, 110);
      target.scrollTop = s0 + ((ev.clientY - y0) * m.max) / Math.max(1, trackH - ph);
    };
    const up = () => {
      dragging = false;
      bar.classList.remove("dragging");
      bar.removeEventListener("pointermove", move);
      bar.removeEventListener("pointerup", up);
      bar.removeEventListener("pointercancel", up);
    };
    bar.addEventListener("pointermove", move);
    bar.addEventListener("pointerup", up);
    bar.addEventListener("pointercancel", up);
  });

  // ganz am rechten Rand verweilen oeffnet sie auch (man muss den duennen Stab nicht treffen)
  const host = doc ? window : sc;
  const onEdge = ((e: PointerEvent) => {
    const m = metrics();
    const near = e.clientX > m.r.right - 18 && e.clientY > m.r.top + insetTop && e.clientY < m.r.bottom - insetBottom;
    if (near && !wide && !openTimer) openTimer = window.setTimeout(() => { openTimer = 0; setWide(true); }, 480);
    if (!near && !bar.matches(":hover")) { clearTimeout(openTimer); openTimer = 0; }
  }) as EventListener;
  host.addEventListener("pointermove", onEdge, { passive: true });

  const onScroll = () => requestAnimationFrame(layout);
  const onResize = () => { if (wide) readSections(); layout(); };
  (doc ? window : sc).addEventListener("scroll", onScroll, { passive: true });
  window.addEventListener("resize", onResize);
  // Inhalt waechst oder schrumpft (Reiter wechseln, Gruppen aufklappen): neu vermessen
  let pending = 0;
  const later = () => { cancelAnimationFrame(pending); pending = requestAnimationFrame(() => { if (wide) readSections(); layout(); }); };
  const ro = new ResizeObserver(later);
  ro.observe(doc ? document.body : sc);
  for (const c of (doc ? document.body : sc).children) ro.observe(c);
  // eigene Aenderungen (Leiste, Marken, Hinweis, Licht auf Glas) nicht mitzaehlen, sonst misst sie sich endlos neu
  const mo = new MutationObserver((list) => {
    if (list.some((r) => !bar.contains(r.target) && !tip.contains(r.target) && !(r.type === "attributes" && (r.target as Element).classList?.contains("n-glass")))) later();
  });
  mo.observe(doc ? document.body : sc, { subtree: true, childList: true, attributes: true, attributeFilter: ["hidden", "open", "class"] });
  layout();
  return {
    layout,
    refresh: () => { readSections(); layout(); },
    /** Leiste entfernen (Bereich wird geschlossen) */
    dispose: () => {
      clearTimeout(openTimer);
      clearTimeout(closeTimer);
      mo.disconnect();
      ro.disconnect();
      host.removeEventListener("pointermove", onEdge);
      (doc ? window : sc).removeEventListener("scroll", onScroll);
      window.removeEventListener("resize", onResize);
      bar.remove();
      tip.remove();
    },
  };
}
