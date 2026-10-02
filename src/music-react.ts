/**
 * Musik-Reaktion: die Kante der schwarzen Notch verformt sich zur Musik.
 *
 * Rust (spectrum.rs) schickt ~30x/s "spectrum": 24 Baender, Pegel, Schlag und erkannten Stil.
 * Hier wird mit requestAnimationFrame eine schwarze Flaeche direkt an die Unterkante (seitlich:
 * Innenkante) gezeichnet — sie sieht aus wie ein Teil der Notch. Bass in der Mitte, Hoehen nach
 * aussen, spiegelsymmetrisch. Kommen keine Daten mehr, klingt alles aus und die Schleife stoppt.
 *
 *   spikes  scharfe Zacken, schnell rauf, schnell runter, Kick drueckt alles tiefer
 *   wave    weiche Welle, traege, mit langsam wanderndem Schwung
 *   pulse   die ganze Kante woelbt sich im Takt
 */
import type { MusicReact } from "./settings-model";

export type Style = "spikes" | "wave" | "pulse";
export type Spectrum = { b: number[]; e: number; k: boolean; s: Style; bpm: number; beat: number };

const BANDS = 24;
const P = BANDS * 2; // Punkte entlang der Kante (gespiegelt)
/** maximale Tiefe in px bei Staerke 1 */
const DEPTH = { compact: 9, expanded: 14 };
const FADE_MS = 600;

type Shape = { v: Float32Array };

export class MusicReactor {
  private ctx: CanvasRenderingContext2D;
  private shapes: Record<Style, Shape> = {
    spikes: { v: new Float32Array(P) },
    wave: { v: new Float32Array(P) },
    pulse: { v: new Float32Array(1) },
  };
  private target = new Float32Array(BANDS);
  private energy = 0;
  private kickEnv = 0;
  private lastData = 0;
  private running = false;
  private style: Style = "pulse";
  private prevStyle: Style | null = null;
  private switchedAt = 0;
  private reduced = matchMedia("(prefers-reduced-motion: reduce)");

  constructor(
    private canvas: HTMLCanvasElement,
    private mode: () => MusicReact,
    private strength: () => number,
    private expanded: () => boolean,
    private dock: () => string,
  ) {
    this.ctx = canvas.getContext("2d")!;
  }

  /** Neues Paket aus Rust */
  feed(d: Spectrum) {
    const m = this.mode();
    if (m === "off" || this.reduced.matches) return;
    for (let i = 0; i < BANDS; i++) this.target[i] = (d.b[i] ?? 0) / 255;
    this.energy = d.e / 255;
    if (d.k) this.kickEnv = 1;
    const next: Style = m === "auto" ? d.s : m;
    if (next !== this.style) {
      this.prevStyle = this.style;
      this.style = next;
      this.switchedAt = performance.now();
    }
    this.lastData = performance.now();
    if (!this.running) {
      this.running = true;
      requestAnimationFrame(this.frame);
    }
  }

  /** Einstellung aus / Vollbild: sofort leeren */
  stop() {
    this.target.fill(0);
    this.energy = 0;
    this.lastData = 0;
  }

  private frame = (now: number) => {
    const fresh = now - this.lastData < 400;
    if (!fresh) {
      // nichts mehr gekommen (Pause, Vollbild, Einstellung aus): ausklingen lassen
      this.target.fill(0);
      this.energy *= 0.9;
    }
    this.kickEnv *= 0.86;
    this.step();
    const alive = fresh || this.peak() > 0.01;
    this.draw(now);
    if (alive && this.mode() !== "off") requestAnimationFrame(this.frame);
    else {
      this.running = false;
      this.ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
    }
  };

  private peak() {
    let m = this.shapes.pulse.v[0];
    for (const s of [this.shapes.spikes, this.shapes.wave]) for (const x of s.v) m = Math.max(m, x);
    return m;
  }

  /** Werte Richtung Ziel bewegen — je Stil eigene Traegheit */
  private step() {
    const sp = this.shapes.spikes.v, wv = this.shapes.wave.v;
    for (let p = 0; p < P; p++) {
      // Mitte = Band 0 (Bass), aussen = hoechstes Band
      const band = Math.min(BANDS - 1, Math.abs(p - (P - 1) / 2) | 0);
      const t = this.target[band];
      // Zacken: schnell hoch, schnell runter, steiler Verlauf
      const ts = Math.pow(t, 1.6);
      sp[p] += (ts - sp[p]) * (ts > sp[p] ? 0.65 : 0.28);
      // Welle: Nachbarn mitteln, traege
      const nb = (this.target[Math.max(0, band - 1)] + t + this.target[Math.min(BANDS - 1, band + 1)]) / 3;
      const tw = 0.75 * nb;
      wv[p] += (tw - wv[p]) * (tw > wv[p] ? 0.12 : 0.05);
    }
    const pv = this.shapes.pulse.v;
    const tp = Math.min(1, 0.45 * this.energy + 0.6 * this.kickEnv);
    pv[0] += (tp - pv[0]) * (tp > pv[0] ? 0.5 : 0.12);
  }

  private draw(now: number) {
    const c = this.canvas, dpr = devicePixelRatio || 1;
    const side = this.dock() !== "top";
    const len = side ? c.clientHeight : c.clientWidth;
    const room = side ? c.clientWidth : c.clientHeight;
    if (c.width !== Math.round(c.clientWidth * dpr) || c.height !== Math.round(c.clientHeight * dpr)) {
      c.width = Math.round(c.clientWidth * dpr);
      c.height = Math.round(c.clientHeight * dpr);
    }
    const g = this.ctx;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, c.clientWidth, c.clientHeight);
    if (len < 4) return;
    const depth = Math.min(room, (this.expanded() ? DEPTH.expanded : DEPTH.compact) * this.strength());
    const fade = Math.min(1, (now - this.switchedAt) / FADE_MS);
    if (this.prevStyle && fade < 1) this.render(this.prevStyle, len, depth, 1 - fade, now, side);
    else this.prevStyle = null;
    this.render(this.style, len, depth, fade, now, side);
  }

  /** Flaeche entlang der Kante: x laeuft die Kante entlang, y geht von der Kante weg */
  private render(style: Style, len: number, depth: number, alpha: number, now: number, side: boolean) {
    const g = this.ctx;
    const pt = (x: number, y: number): [number, number] => {
      if (!side) return [x, y];
      return this.dock() === "left" ? [y, x] : [this.canvas.clientWidth - y, x];
    };
    // an den Enden auslaufen lassen, damit die Flaeche sauber in die runden Ecken uebergeht
    const taper = (u: number) => Math.pow(Math.sin(Math.PI * Math.min(1, Math.max(0, u))), 0.6);
    const kick = 1 + 0.35 * this.kickEnv;
    const pts: [number, number][] = [];
    if (style === "pulse") {
      const v = this.shapes.pulse.v[0];
      const n = 32;
      for (let i = 0; i <= n; i++) {
        const u = i / n;
        pts.push([u * len, depth * v * Math.pow(Math.sin(Math.PI * u), 1.2)]);
      }
    } else {
      const v = style === "spikes" ? this.shapes.spikes.v : this.shapes.wave.v;
      for (let p = 0; p < P; p++) {
        const u = (p + 0.5) / P;
        let d = v[p];
        if (style === "wave") {
          // langsam wandernder Schwung, damit ruhige Musik nicht starr wirkt
          d = d * 0.75 + 0.25 * this.energy * (0.5 + 0.5 * Math.sin(now / 700 + p * 0.45));
          pts.push([u * len, depth * 0.8 * d * taper(u)]);
        } else {
          pts.push([u * len, depth * d * kick * taper(u)]);
          // zwischen zwei Zacken zurueck zur Kante -> echte Spitzen statt Huegel
          if (p < P - 1) pts.push([((p + 1) / P) * len, depth * 0.15 * d * taper(u)]);
        }
      }
    }
    g.globalAlpha = alpha;
    g.fillStyle = "#000";
    g.beginPath();
    g.moveTo(...pt(0, 0));
    if (style === "wave") {
      // weiche Kurve durch die Mittelpunkte
      let prev: [number, number] = [0, 0];
      for (const q of pts) {
        g.quadraticCurveTo(...pt(prev[0], prev[1]), ...pt((prev[0] + q[0]) / 2, (prev[1] + q[1]) / 2));
        prev = q;
      }
      g.quadraticCurveTo(...pt(prev[0], prev[1]), ...pt(len, 0));
    } else {
      for (const q of pts) g.lineTo(...pt(q[0], q[1]));
      g.lineTo(...pt(len, 0));
    }
    // ein Pixel unter die Notch greifen, damit keine Haarlinie zwischen Form und Flaeche bleibt
    g.lineTo(...pt(len, -1));
    g.lineTo(...pt(0, -1));
    g.closePath();
    g.fill();
    g.globalAlpha = 1;
  }
}
