/**
 * Musik-Reaktion: die schwarze Notch verformt sich zur Musik — an allen drei freien Seiten.
 *
 * Rust (spectrum.rs) schickt ~30x/s "spectrum": 24 Baender, Pegel, Schlag und die Anteile der drei
 * Stile. Hier liegt eine fensterfuellende Leinwand HINTER der Form; gezeichnet wird eine schwarze
 * Flaeche, deren Rand der Kontur der Notch folgt (links runter, unten quer, rechts hoch; seitlich
 * angedockt entsprechend gedreht) und nach aussen ausschlaegt. Bass unten in der Mitte, Hoehen zu
 * den Enden, die zur Bildschirmkante hin auslaufen.
 *
 *   Zacken  scharfe Spitzen, schnell rauf und runter, Kick drueckt alles weiter raus
 *   Welle   weiche Welle, traege, mit langsam wanderndem Schwung
 *   Puls    die ganze Kontur woelbt sich im Takt
 *
 * Die Stile ueberlagern sich: pro Punkt zaehlt der groesste gewichtete Ausschlag.
 */
import type { Layer, Layers, MusicReact } from "./settings-model";

export type Spectrum = { b: number[]; e: number; k: boolean; s: Layer; bpm: number; beat: number; w?: number[] };

const BANDS = 24;
/** maximaler Ausschlag in px bei Staerke 100 % */
const DEPTH = { compact: 26, expanded: 54 };
/** zur Bildschirmkante hin auf so vielen px auslaufen (dort sitzen die Ohren) */
const EDGE_FADE = 8;
/** hoechstes Band, das an der Kontur ankommt (~2,7 kHz): darueber ist Musik fast immer leise */
const TOP_BAND = 17;
/** Abstand der Zacken entlang der Kontur in px */
const SPIKE_GAP = 13;
/** Abtastabstand der Kontur in px */
const STEP = 3;

export class MusicReactor {
  private ctx: CanvasRenderingContext2D;
  private target = new Float32Array(BANDS);
  private spikes = new Float32Array(BANDS);
  private wave = new Float32Array(BANDS);
  private pulse = 0;
  private energy = 0;
  private kickEnv = 0;
  /** Anteile Zacken/Welle/Puls: Ziel aus Rust bzw. Einstellung, aktuell weich nachgefuehrt */
  private wTarget: [number, number, number] = [0, 0, 1];
  private w: [number, number, number] = [0, 0, 0];
  private lastData = 0;
  private running = false;
  private reduced = matchMedia("(prefers-reduced-motion: reduce)");

  constructor(
    private canvas: HTMLCanvasElement,
    private shape: HTMLElement,
    private mode: () => MusicReact,
    private layers: () => Layers,
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
    if (m === "auto") {
      const w = d.w ?? [0, 0, 100];
      this.wTarget = [w[0] / 100, w[1] / 100, w[2] / 100];
    } else {
      const l = this.layers();
      this.wTarget = [+l.spikes, +l.wave, +l.pulse];
    }
    this.lastData = performance.now();
    if (!this.running) {
      this.running = true;
      requestAnimationFrame(this.frame);
    }
  }

  /** Einstellung aus: sofort leeren */
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
      this.ctx.setTransform(1, 0, 0, 1, 0, 0);
      this.ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
    }
  };

  private peak() {
    let m = this.pulse;
    for (let i = 0; i < BANDS; i++) m = Math.max(m, this.spikes[i], this.wave[i]);
    return m;
  }

  /** Werte Richtung Ziel bewegen — je Stil eigene Traegheit */
  private step() {
    for (let i = 0; i < BANDS; i++) {
      const t = this.target[i];
      const ts = Math.pow(t, 1.25);
      this.spikes[i] += (ts - this.spikes[i]) * (ts > this.spikes[i] ? 0.65 : 0.28);
      const nb = (this.target[Math.max(0, i - 1)] + t + this.target[Math.min(BANDS - 1, i + 1)]) / 3;
      const tw = 0.8 * nb;
      this.wave[i] += (tw - this.wave[i]) * (tw > this.wave[i] ? 0.12 : 0.05);
    }
    const tp = Math.min(1, 0.45 * this.energy + 0.6 * this.kickEnv);
    this.pulse += (tp - this.pulse) * (tp > this.pulse ? 0.5 : 0.12);
    // Stil-Anteile weich ueberblenden (~1 s)
    for (let i = 0; i < 3; i++) this.w[i] += (this.wTarget[i] - this.w[i]) * 0.05;
  }

  /** Band-Wert an Position u (0..1 entlang der Kontur): Mitte = Bass, Enden = Mitten/Hoehen, linear interpoliert */
  private band(arr: Float32Array, u: number) {
    const x = Math.min(1, Math.abs(u - 0.5) * 2) * TOP_BAND;
    const i = Math.floor(x), f = x - i;
    return arr[i] * (1 - f) + arr[Math.min(TOP_BAND, i + 1)] * f;
  }

  private draw(now: number) {
    const c = this.canvas, dpr = devicePixelRatio || 1;
    const cw = c.clientWidth, ch = c.clientHeight;
    if (c.width !== Math.round(cw * dpr) || c.height !== Math.round(ch * dpr)) {
      c.width = Math.round(cw * dpr);
      c.height = Math.round(ch * dpr);
    }
    const g = this.ctx;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, cw, ch);

    // Kontur der Form im Fenster (weggefahren: nichts zeichnen)
    if (this.shape.closest(".gone")) return;
    const r = this.shape.getBoundingClientRect();
    const cr = c.getBoundingClientRect();
    if (r.width < 4 || r.height < 4) return;
    const dock = this.dock();
    const side = dock !== "top";
    // lokales Bild: entlang (W) mal Tiefe (H), Ursprung an der Bildschirmkante
    const W = side ? r.height : r.width;
    const H = side ? r.width : r.height;
    // die runden Ecken liegen je nach Andockseite links/rechts unten -> den groesseren nehmen
    const cs = getComputedStyle(this.shape);
    const rad = Math.min(Math.max(parseFloat(cs.borderBottomLeftRadius) || 0, parseFloat(cs.borderBottomRightRadius) || 0, 1), W / 2, H);
    const ox = r.left - cr.left, oy = r.top - cr.top;
    const toScreen = (x: number, y: number): [number, number] =>
      dock === "left" ? [ox + y, oy + x] : dock === "right" ? [ox + r.width - y, oy + x] : [ox + x, oy + y];

    const amp = (this.expanded() ? DEPTH.expanded : DEPTH.compact) * this.strength();
    const kick = 1 + 0.35 * this.kickEnv;
    const [ws, ww, wp] = this.w;

    // Kontur abtasten: links runter (Seite), Ecke, unten, Ecke, rechts hoch
    const sideLen = Math.max(0, H - rad), arc = (Math.PI / 2) * rad, bottom = Math.max(0, W - 2 * rad);
    const L = 2 * sideLen + 2 * arc + bottom;
    const n = Math.max(8, Math.ceil(L / STEP));
    const spikesN = Math.max(6, Math.round(L / SPIKE_GAP));
    const at = (s: number): [number, number, number, number] => {
      // Punkt (x, y) und Aussennormale (nx, ny) bei Bogenlaenge s
      if (s <= sideLen) return [0, s, -1, 0];
      s -= sideLen;
      if (s <= arc) { const a = s / rad; return [rad - rad * Math.cos(a), sideLen + rad * Math.sin(a), -Math.cos(a), Math.sin(a)]; }
      s -= arc;
      if (s <= bottom) return [rad + s, H, 0, 1];
      s -= bottom;
      if (s <= arc) { const a = s / rad; return [W - rad + rad * Math.sin(a), H - rad + rad * Math.cos(a), Math.sin(a), Math.cos(a)]; }
      s -= arc;
      return [W, H - rad - s, 1, 0];
    };

    g.fillStyle = "#000";
    g.beginPath();
    g.moveTo(...toScreen(0, 0));
    for (let k = 0; k <= n; k++) {
      const u = k / n;
      const [x, y, nx, ny] = at(u * L);
      // nur direkt an der Bildschirmkante auslaufen (dort sitzen die Ohren)
      const taper = Math.min(1, (u * L) / EDGE_FADE, ((1 - u) * L) / EDGE_FADE);
      // Zacken: Dreieck je Zelle, Spitze in der Mitte
      const cell = u * spikesN, tri = 1 - Math.abs((cell - Math.floor(cell)) * 2 - 1);
      const dSpk = this.band(this.spikes, (Math.floor(cell) + 0.5) / spikesN) * kick * (0.15 + 0.85 * tri);
      const dWav = this.band(this.wave, u) * 0.75 + 0.25 * this.energy * (0.5 + 0.5 * Math.sin(now / 700 + u * 18));
      const dPul = this.pulse;
      const d = amp * taper * Math.max(ws * dSpk, ww * dWav, wp * dPul);
      g.lineTo(...toScreen(x + nx * d, y + ny * d));
    }
    g.lineTo(...toScreen(W, 0));
    g.closePath();
    g.fill();
  }
}
