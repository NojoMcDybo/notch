/**
 * Karten in der aufgeklappten Notch (Musik, Blutzucker, LoL, Sport, Zeilen anderer Apps):
 * eigene Reihenfolge per Ziehen, weggeklickte Karten bleiben weg, bis man sie ueber „+“ zurueckholt.
 *
 * - Die Reihenfolge laeuft ueber CSS `order` — keine DOM-Verschiebung, Eingabefelder behalten den Fokus.
 * - Gemerkt wird nur die Reihenfolge der Schluessel (`data-card`, meist die Activity-id). Neue Karten
 *   stehen an ihrer natuerlichen Stelle, gemerkte nehmen untereinander die gemerkte Reihenfolge ein.
 * - Ziehen: ab 6 px Bewegung hebt sich die Karte an und folgt der Maus, die anderen gleiten weich
 *   auf ihren neuen Platz (FLIP); beim Loslassen federt sie ein.
 * - Gespeichert in config.json -> "cards" (lib.rs: cards_get / cards_set).
 */

export type HiddenCard = { id: string; label: string; seen: number };
export type CardsState = { order: string[]; hidden: HiddenCard[] };

/** so lange (ohne dass die Karte noch einmal auftaucht) merkt sich die Notch weggeklickte Karten */
const FORGET_MS = 30 * 86_400_000;

export function normalizeCards(raw: unknown): CardsState {
  const r = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
  const str = (x: unknown): x is string => typeof x === "string" && x.length > 0 && x.length < 200;
  const order = [...new Set((Array.isArray(r.order) ? r.order : []).filter(str))].slice(0, 80);
  const now = Date.now();
  const hidden = (Array.isArray(r.hidden) ? r.hidden : [])
    .map((h) => (h && typeof h === "object" ? (h as Record<string, unknown>) : {}))
    .filter((h) => str(h.id))
    .map((h) => ({ id: String(h.id), label: typeof h.label === "string" ? h.label.slice(0, 80) : String(h.id), seen: Number(h.seen) || now }))
    .filter((h, i, a) => a.findIndex((x) => x.id === h.id) === i && now - h.seen < FORGET_MS)
    .slice(0, 40);
  return { order, hidden };
}

/**
 * Reihenfolge der Zeilen ohne Springen: Rust sortiert nach Prioritaet und letzter Aktualisierung, dann rutschte jede
 * Karte bei jeder Meldung nach oben (Folio beim Durchklicken der Treffer, Haze jede Minute). Vorhandene behalten
 * ihren Platz; neue kommen an die Stelle, die sie in der Liste haben.
 */
export function stableOrder(prev: string[], ids: string[]) {
  const live = new Set(ids);
  const out = prev.filter((id) => live.has(id));
  ids.forEach((id, i) => { if (!out.includes(id)) out.splice(Math.min(i, out.length), 0, id); });
  return out;
}

const key = (e: HTMLElement) => e.dataset.card ?? "";
/** grosse Karten bekommen eine Trennlinie; kleine Zeilen untereinander nur Abstand */
const big = (e: HTMLElement) => !e.classList.contains("act") || e.classList.contains("bg") || e.classList.contains("pomo");
/** hier beginnt kein Ziehen (Knoepfe, Regler, Eingabefelder) */
const NO_DRAG = "button, input, .slider, .act-input, .wheel, .ctx";
/** Layout-Klassen, die ein Neuaufbau der Zeile nicht wegwischen darf */
export const LAYOUT_CLS = ["card", "sep", "first", "enter", "lifting"];

const spring = () => getComputedStyle(document.documentElement).getPropertyValue("--spring").trim() || "ease-out";

type Drag = { el: HTMLElement; grab: number; y: number; moved: boolean };

export class Deck {
  /** sichtbare Karten in der angezeigten Reihenfolge */
  private seq: HTMLElement[] = [];
  private shown = new Set<HTMLElement>();
  private ready = false;
  private drag: Drag | null = null;

  constructor(
    private box: HTMLElement,
    private o: {
      /** neue Reihenfolge der sichtbaren Karten (nach dem Loslassen) */
      onOrder: (keys: string[]) => void;
      /** Notch offen halten, solange gezogen wird */
      hold: (on: boolean) => void;
    },
  ) {
    box.addEventListener("pointerdown", (e) => this.down(e));
  }

  get dragging() {
    return !!this.drag;
  }

  /** `natural`: sichtbare Karten in ihrer natuerlichen Reihenfolge; `stored`: gemerkte Reihenfolge */
  layout(natural: HTMLElement[], stored: string[]) {
    let seq: HTMLElement[];
    if (this.drag) {
      // waehrend des Ziehens bleibt die gezogene Reihenfolge; Neues haengt hinten an
      const live = new Set(natural);
      seq = this.seq.filter((e) => live.has(e));
      for (const e of natural) if (!seq.includes(e)) seq.push(e);
    } else {
      const rank = new Map(stored.map((k, i) => [k, i]));
      const known = natural.filter((e) => rank.has(key(e))).sort((a, b) => rank.get(key(a))! - rank.get(key(b))!);
      let k = 0;
      seq = natural.map((e) => (rank.has(key(e)) ? known[k++] : e));
    }
    this.apply(seq);
  }

  private apply(seq: HTMLElement[]) {
    this.seq = seq;
    seq.forEach((e, i) => {
      e.classList.add("card");
      e.style.order = String(i);
      e.classList.toggle("first", i === 0);
      e.classList.toggle("sep", i > 0 && (big(e) || big(seq[i - 1])));
    });
    // neu sichtbar (neue Activity, zurueckgeholt): weich einblenden — nicht beim ersten Aufbau
    const now = new Set(seq);
    if (this.ready && !matchMedia("(prefers-reduced-motion: reduce)").matches) {
      for (const e of seq) {
        if (this.shown.has(e)) continue;
        e.classList.remove("enter");
        void e.offsetWidth;
        e.classList.add("enter");
        e.addEventListener("animationend", () => e.classList.remove("enter"), { once: true });
      }
    }
    this.shown = now;
    this.ready = true;
  }

  /** Karte weich ausblenden, danach `done` (dort wird sie wirklich versteckt) */
  static fadeOut(e: HTMLElement, done: () => void) {
    if (matchMedia("(prefers-reduced-motion: reduce)").matches) { done(); return; }
    e.animate([{ opacity: 1, scale: "1" }, { opacity: 0, scale: "0.96" }], { duration: 170, easing: "ease-in", fill: "forwards" })
      .finished.then(() => { finish(); }, finish);
    let fired = false;
    function finish() { if (fired) return; fired = true; done(); e.getAnimations().forEach((a) => a.cancel()); }
    setTimeout(finish, 300);
  }

  // ---------- Ziehen ----------

  private down(e: PointerEvent) {
    const t = e.target as Element;
    const el = t.closest<HTMLElement>(".card");
    if (e.button !== 0 || !el || t.closest(NO_DRAG) || !this.seq.includes(el) || this.seq.length < 2) return;
    const x0 = e.clientX, y0 = e.clientY;
    const mv = (ev: PointerEvent) => {
      if (!this.drag) {
        if (Math.hypot(ev.clientX - x0, ev.clientY - y0) < 6) return;
        this.start(el, y0);
      }
      this.follow(ev.clientY);
    };
    const up = () => {
      window.removeEventListener("pointermove", mv);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
      if (this.drag) this.end();
    };
    window.addEventListener("pointermove", mv);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
  }

  private start(el: HTMLElement, y0: number) {
    el.getAnimations().forEach((a) => a.cancel());
    this.drag = { el, grab: y0 - el.getBoundingClientRect().top, y: y0, moved: false };
    el.classList.add("lifting");
    this.box.classList.add("dragging");
    this.o.hold(true);
  }

  /** gezogene Karte unter die Maus legen, Nachbarn weichen aus */
  private follow(y: number) {
    const d = this.drag!;
    const down = y >= d.y;
    d.y = y;
    const box = this.box.getBoundingClientRect();
    const h = d.el.offsetHeight;
    const top = Math.max(-8, Math.min(this.box.offsetHeight - h + 8, y - box.top - d.grab));
    d.el.style.translate = `0 ${top - d.el.offsetTop}px`;
    const others = this.seq.filter((e) => e !== d.el);
    // vordere Kante in Zugrichtung gegen die Mitte der anderen — so kommt auch eine hohe Karte ganz nach oben/unten
    const edge = down ? top + h : top;
    const idx = others.filter((o) => o.offsetTop + o.offsetHeight / 2 < edge).length;
    if (idx !== this.seq.indexOf(d.el)) {
      d.moved = true;
      this.moveTo(idx);
      d.el.style.translate = `0 ${top - d.el.offsetTop}px`;
    }
  }

  private moveTo(i: number) {
    const el = this.drag!.el;
    const others = this.seq.filter((e) => e !== el);
    const before = new Map(others.map((e) => [e, e.getBoundingClientRect().top]));
    for (const e of others) e.getAnimations().forEach((a) => { if (a.id === "flip") a.cancel(); });
    const seq = [...others];
    seq.splice(i, 0, el);
    this.apply(seq);
    for (const e of others) {
      const dy = before.get(e)! - e.getBoundingClientRect().top;
      if (Math.abs(dy) < 0.5) continue;
      const an = e.animate([{ translate: `0 ${dy}px` }, { translate: "0 0" }], { duration: 420, easing: spring() });
      an.id = "flip";
    }
  }

  private end() {
    const d = this.drag!;
    this.drag = null;
    const el = d.el;
    const from = el.style.translate || "0 0";
    el.style.translate = "";
    const an = el.animate([{ translate: from, scale: "1.02" }, { translate: "0 0", scale: "1" }], { duration: 460, easing: spring() });
    const done = () => { el.classList.remove("lifting"); this.box.classList.remove("dragging"); };
    an.finished.then(done, done);
    setTimeout(done, 600); // falls keine Bilder gezeichnet werden (Fenster verdeckt), trotzdem aufraeumen
    // der Klick nach dem Loslassen soll die Karte nicht oeffnen
    const stop = (e: Event) => { e.stopPropagation(); e.preventDefault(); };
    window.addEventListener("click", stop, { capture: true, once: true });
    setTimeout(() => window.removeEventListener("click", stop, { capture: true }), 60);
    // erst merken, dann freigeben: hold(false) zeichnet neu und wuerde sonst die alte Reihenfolge anwenden
    if (d.moved) this.o.onOrder(this.seq.map(key));
    this.o.hold(false);
  }
}
