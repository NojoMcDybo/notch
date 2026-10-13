/**
 * Sprachassistent als Kreis neben der Notch. Bei Aktivierung loest er sich fluessig aus der Notch (oben angedockt
 * nach rechts, seitlich nach unten), Durchmesser = Hoehe der eingeklappten Notch; am Ende fliesst er zurueck.
 *
 * Fluessig: schwarze Formen in einer eigenen Ebene mit SVG-Filter (#goo: weichzeichnen, dann hart abschneiden).
 * Ein Stueck Notch-Rand („Stummel“, liegt unter der Notch) und der Kreis verschmelzen, ziehen sich lang und reissen
 * ab. Die Ebene liegt hinter der Notch, die Anzeige im Kreis (Knopf) davor. Je Zustand eine eigene Animation
 * (bubble.css): verbinden, zuhoeren (Mikrofonpegel), nachdenken, arbeiten (Werkzeuge laufen), sprechen (Stimme).
 * Klick auf den Kreis beendet den Assistenten.
 */
import "./bubble.css";

export type BubbleState = "connecting" | "listening" | "thinking" | "acting" | "speaking";
type Box = { x: number; y: number; w: number; h: number; r: number };
type Rect = { x: number; y: number; w: number; h: number };

/** Abstand zwischen Notch (samt Hohlkehle) und Kreis */
const GAP = 10;
/** so lange laeuft das Zurueckfliessen, danach wird die Ebene ausgeblendet */
const BACK_MS = 520;

export class Bubble {
  private goo: HTMLElement;
  private btn: HTMLButtonElement;
  private bars: HTMLElement[];
  private on = false;
  private hideTimer = 0;
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
    this.goo.innerHTML = '<i class="g-stub"></i><i class="g-blob"></i>';
    this.btn = document.createElement("button");
    this.btn.className = "vbub";
    this.btn.title = "Assistent beenden";
    this.btn.setAttribute("aria-label", "Sprachassistent beenden");
    this.btn.innerHTML = '<span class="vb-ring"></span><span class="vb-core"></span><span class="vb-bars"><i></i><i></i><i></i><i></i></span>';
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
    const css = (el: HTMLElement, v: Record<string, string>) => Object.entries(v).forEach(([k, x]) => el.style.setProperty(k, x));
    const px = (v: number) => `${Math.round(v * 10) / 10}px`;
    const vars = { "--d": px(d), "--hx": px(home.x), "--hy": px(home.y), "--tx": px(target.x), "--ty": px(target.y) };
    css(this.goo, { ...vars, "--sx": px(stub.x), "--sy": px(stub.y), "--sw": px(stub.w), "--sh": px(stub.h), "--sr": stub.r });
    css(this.btn, vars);
    this.btn.dataset.s = st;

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
      } else {
        this.goo.classList.remove("out");
        this.btn.classList.remove("out");
        this.hideTimer = window.setTimeout(() => this.goo.classList.remove("live"), BACK_MS);
      }
    }
    return on ? { x: target.x - d / 2, y: target.y - d / 2, w: d, h: d } : null;
  }

  /** Mikrofonpegel 0..1 (zuhoeren) */
  level(l: number) {
    this.btn.style.setProperty("--lvl", l.toFixed(3));
  }

  /** Pegel der Stimme 0..1 (sprechen): vier Balken, leicht versetzt */
  out(l: number) {
    if (this.btn.dataset.s !== "speaking") return;
    this.phase += 0.35;
    const shape = [0.6, 1, 0.85, 0.55];
    this.bars.forEach((b, i) => {
      const wob = 0.75 + 0.25 * Math.sin(this.phase + i * 1.4);
      b.style.setProperty("--s", Math.max(0.18, Math.min(1, l * 1.7 * shape[i] * wob)).toFixed(3));
    });
  }
}
