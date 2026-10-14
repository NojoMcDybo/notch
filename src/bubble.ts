/**
 * Sprachassistent als Kreis neben der Notch. Bei Aktivierung loest er sich fluessig aus der Notch (oben angedockt
 * nach rechts, seitlich nach unten), Durchmesser = Hoehe der eingeklappten Notch; am Ende fliesst er zurueck.
 *
 * Fluessig: schwarze Formen in einer eigenen Ebene mit SVG-Filter (#goo: weichzeichnen, dann hart abschneiden).
 * Ein Stueck Notch-Rand („Stummel“, liegt unter der Notch) und der Kreis verschmelzen, ziehen sich lang und reissen
 * ab. Die Ebene liegt hinter der Notch, die Anzeige im Kreis (Knopf) davor. Schwarz-weiss, je Zustand ein eigenes
 * Punktmuster (bubble.css): verbinden (Ladekreis), zuhoeren (Ring weitet sich mit dem Mikrofon), nachdenken (Schweif
 * kreist), arbeiten (vier Punkte pumpen und drehen), sprechen (Balken). Der Kreis selbst atmet mit der Stimme und
 * staucht sich bei jedem Zustandswechsel kurz.
 * Klick auf den Kreis beendet den Assistenten.
 */
import "./bubble.css";

export type BubbleState = "connecting" | "listening" | "thinking" | "acting" | "speaking";
type Box = { x: number; y: number; w: number; h: number; r: number };
type Rect = { x: number; y: number; w: number; h: number };

/** Abstand zwischen Notch (samt Hohlkehle) und Kreis */
const GAP = 10;
/** so lange laeuft das Zurueckfliessen, danach wird die Ebene ausgeblendet */
const BACK_MS = 700;
/** Dauer des Abtropfens bzw. Zuruecksaugens */
const OUT_MS = 1150;
const IN_MS = 680;

export class Bubble {
  private goo: HTMLElement;
  private btn: HTMLButtonElement;
  private bars: HTMLElement[];
  private on = false;
  private hideTimer = 0;
  private blob!: HTMLElement;
  private bulge!: HTMLElement;
  private shape: HTMLElement | null;
  /** Bewegung vom Ausgangspunkt zum Ziel (fuer die Tropfen-Animation) und Richtung (oben: x, seitlich: y) */
  private dx = 0;
  private dy = 0;
  private along = "x";
  private phase = 0;

  constructor(notch: HTMLElement, onClick: () => void) {
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.setAttribute("class", "goo-defs");
    svg.setAttribute("aria-hidden", "true");
    svg.innerHTML =
      '<filter id="goo" x="-20%" y="-20%" width="140%" height="140%" color-interpolation-filters="sRGB">' +
      '<feGaussianBlur in="SourceGraphic" stdDeviation="5"/>' +
      '<feColorMatrix values="0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 24 -10"/></filter>';
    this.goo = document.createElement("div");
    this.goo.className = "vgoo";
    this.goo.setAttribute("aria-hidden", "true");
    this.goo.innerHTML = '<i class="g-stub"></i><i class="g-bulge"></i><i class="g-blob"></i>';
    this.blob = this.goo.querySelector<HTMLElement>(".g-blob")!;
    this.bulge = this.goo.querySelector<HTMLElement>(".g-bulge")!;
    this.shape = notch.querySelector<HTMLElement>(".shape");
    this.btn = document.createElement("button");
    this.btn.className = "vbub";
    this.btn.title = "Assistent beenden";
    this.btn.setAttribute("aria-label", "Sprachassistent beenden");
    const dots = Array.from({ length: 8 }, (_, i) => `<b style="--i:${i}"></b>`).join("");
    this.btn.innerHTML = `<span class="vb-dots">${dots}</span><span class="vb-bars"><i></i><i></i><i></i><i></i></span>`;
    this.btn.addEventListener("click", onClick);
    this.bars = [...this.btn.querySelectorAll<HTMLElement>(".vb-bars i")];
    // Ebene vor der Notch im DOM = dahinter gemalt; Knopf danach = davor
    notch.before(svg, this.goo);
    notch.after(this.btn);
  }

  /**
   * Lage und Zustand setzen. `n`: Form der Notch im Fenster (ohne Hohlkehlen), `ear`: Breite der Hohlkehle,
   * `d`: Durchmesser. Gibt die Flaeche des Kreises zurueck (fuer den Mausfangbereich), wenn er sichtbar ist.
   */
  place(on: boolean, st: BubbleState, n: Box, dock: string, ear: number, d: number): Rect | null {
    const top = dock === "top";
    // Ausgangspunkt knapp innerhalb der Notch, Ziel daneben (oben: rechts, seitlich: darunter)
    const home = top ? { x: n.x + n.w - d / 2 - 4, y: d / 2 } : { x: n.x + n.w / 2, y: n.y + n.h - d / 2 - 4 };
    const target = top ? { x: n.x + n.w + ear + GAP + d / 2, y: d / 2 } : { x: n.x + n.w / 2, y: n.y + n.h + ear + GAP + d / 2 };
    // Stummel: das Ende der Notch, an dem der Kreis haengt
    const stub = top
      ? { x: n.x + n.w - d - 12, y: n.y, w: d + 12, h: n.h, r: `0 0 ${n.r}px 0` }
      : { x: n.x, y: n.y + n.h - d - 12, w: n.w, h: d + 12, r: dock === "left" ? `0 0 ${n.r}px 0` : `0 0 0 ${n.r}px` };
    // Wulst am Notch-Rand: aus ihm waechst der Tropfen (oben: Mitte der rechten Kante, seitlich: Mitte unten)
    const bulge = top ? { x: n.x + n.w, y: n.h / 2 } : { x: n.x + n.w / 2, y: n.y + n.h };
    this.dx = target.x - home.x;
    this.dy = target.y - home.y;
    this.along = top ? "x" : "y";
    const css = (el: HTMLElement, v: Record<string, string>) => Object.entries(v).forEach(([k, x]) => el.style.setProperty(k, x));
    const px = (v: number) => `${Math.round(v * 10) / 10}px`;
    const vars = { "--d": px(d), "--hx": px(home.x), "--hy": px(home.y), "--tx": px(target.x), "--ty": px(target.y) };
    css(this.goo, { ...vars, "--sx": px(stub.x), "--sy": px(stub.y), "--sw": px(stub.w), "--sh": px(stub.h), "--sr": stub.r, "--bx": px(bulge.x), "--by": px(bulge.y) });
    css(this.btn, vars);
    if (this.btn.dataset.s !== st) {
      // Rueckmeldung beim Wechsel: Kreis staucht sich kurz (Animation neu starten)
      if (this.on && on) {
        this.goo.classList.remove("kick");
        void this.goo.offsetWidth;
        this.goo.classList.add("kick");
      }
      this.btn.dataset.s = st;
      if (st !== "listening" && st !== "speaking") this.amp(0);
    }

    if (on !== this.on) {
      this.on = on;
      clearTimeout(this.hideTimer);
      if (on) {
        // erst ohne Uebergang an den Ausgangspunkt (solange die Ebene versteckt war, hat der Browser alte Werte
        // behalten), dann hinaus
        this.goo.classList.add("live", "snap");
        void this.goo.offsetWidth;
        this.goo.classList.remove("snap");
        this.goo.classList.add("out");
        this.btn.classList.add("out");
        this.drip(true);
      } else {
        this.goo.classList.remove("out");
        this.btn.classList.remove("out");
        this.drip(false);
        this.hideTimer = window.setTimeout(() => this.goo.classList.remove("live"), BACK_MS);
      }
    }
    return on ? { x: target.x - d / 2, y: target.y - d / 2, w: d, h: d } : null;
  }

  /**
   * Tropfen: hinaus waechst er aus einem Wulst am Notch-Rand, zieht sich lang, der Hals reisst (der SVG-Filter
   * macht den Hals), er schwingt ueber und rundet sich; die Notch schwillt dabei leicht an und federt beim Abreissen
   * zurueck. Hinein: kurz ausholen, lang ziehen, die Notch schluckt ihn mit einem kleinen Ruck.
   * Bewegt wird ueber `transform` relativ zum Ruhepunkt der CSS-Klasse — so bleiben `translate` (Lage) und
   * `scale` (Atmen mit der Stimme) unberuehrt.
   */
  private drip(out: boolean) {
    if (matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const x = this.along === "x";
    // Lage als Anteil des Wegs (0 = Ausgangspunkt, 1 = Ziel) relativ zur Ruhelage der Klasse
    const at = (f: number, sa: number, sc: number) => {
      const rest = out ? 1 : 0;
      const tx = (f - rest) * this.dx, ty = (f - rest) * this.dy;
      // gedehnt in Bewegungsrichtung: sa laengs, sc quer
      return `translate(${tx.toFixed(1)}px, ${ty.toFixed(1)}px) scale(${x ? sa : sc}, ${x ? sc : sa})`;
    };
    const k = (offset: number, f: number, sa: number, sc: number) => ({ offset, transform: at(f, sa, sc) });
    const blob = out
      ? [k(0, 0, 0.5, 0.5), k(0.22, 0.12, 0.92, 0.88), k(0.42, 0.42, 1.28, 0.8), k(0.56, 0.82, 1.2, 0.84), k(0.68, 1.07, 0.84, 1.15), k(0.82, 0.98, 1.06, 0.95), k(1, 1, 1, 1)]
      : [k(0, 1, 1, 1), k(0.22, 1.05, 0.88, 1.1), k(0.6, 0.35, 1.25, 0.84), k(0.86, 0.06, 0.8, 0.8), k(1, 0, 0.5, 0.5)];
    const bulge = out
      ? [{ offset: 0, transform: "scale(0)" }, { offset: 0.3, transform: "scale(1)" }, { offset: 0.5, transform: "scale(0.85)" }, { offset: 0.6, transform: "scale(0.2)" }, { offset: 0.74, transform: "scale(0.42)" }, { offset: 1, transform: "scale(0)" }]
      : [{ offset: 0, transform: "scale(0)" }, { offset: 0.55, transform: "scale(0.35)" }, { offset: 0.82, transform: "scale(1)" }, { offset: 1, transform: "scale(0)" }];
    const sw = (a: number, c: number) => (x ? `${a} ${c}` : `${c} ${a}`);
    const notch = out
      ? [{ offset: 0, scale: "1" }, { offset: 0.3, scale: sw(1.006, 1.035) }, { offset: 0.58, scale: sw(0.993, 0.965) }, { offset: 0.72, scale: sw(1.004, 1.012) }, { offset: 1, scale: "1" }]
      : [{ offset: 0, scale: "1" }, { offset: 0.8, scale: sw(1.008, 1.045) }, { offset: 0.9, scale: sw(0.996, 0.985) }, { offset: 1, scale: "1" }];
    const t = { duration: out ? OUT_MS : IN_MS, easing: out ? "cubic-bezier(0.4, 0, 0.25, 1)" : "cubic-bezier(0.5, 0, 0.6, 1)" };
    this.blob.animate(blob, t);
    this.bulge.animate(bulge, t);
    this.shape?.animate(notch, t);
  }

  /** Kreis atmet mit: 0..1 */
  private amp(a: number) {
    const v = a.toFixed(3);
    this.goo.style.setProperty("--amp", v);
    this.btn.style.setProperty("--amp", v);
  }

  /** Mikrofonpegel 0..1 (zuhoeren) */
  level(l: number) {
    this.btn.style.setProperty("--lvl", l.toFixed(3));
    if (this.btn.dataset.s === "listening") this.amp(l);
  }

  /** Pegel der Stimme 0..1 (sprechen): vier Balken, leicht versetzt */
  out(l: number) {
    if (this.btn.dataset.s !== "speaking") return;
    this.amp(l);
    this.phase += 0.35;
    const shape = [0.6, 1, 0.85, 0.55];
    this.bars.forEach((b, i) => {
      const wob = 0.75 + 0.25 * Math.sin(this.phase + i * 1.4);
      b.style.setProperty("--b", Math.max(0.18, Math.min(1, l * 1.7 * shape[i] * wob)).toFixed(3));
    });
  }
}
