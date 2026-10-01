/**
 * Drehrad wie in der iPhone-Uhr: Ziehen mit Schwung, Mausrad = ein Schritt, Klick auf eine
 * Zahl springt hin. Endlos (nach 59 kommt 0). Gezeichnet wird per transform, nicht per
 * Scrollen — so rastet es sauber ein und laesst sich auch mit der Maus ziehen.
 */

const ROW = 30; // px je Zeile
const SHOW = 3.2; // so viele Zeilen ober-/unterhalb sind sichtbar

export class Wheel {
  readonly el: HTMLElement;
  private items: HTMLElement[] = [];
  private value: number; // Position als Kommazahl (Index in der Mitte)
  private anim = 0;

  constructor(private n: number, initial: number, unit: string, private onHold: (on: boolean) => void) {
    this.value = initial;
    this.el = document.createElement("div");
    this.el.className = "wheel";
    const track = document.createElement("div");
    track.className = "w-track";
    for (let i = 0; i < n; i++) {
      const it = document.createElement("div");
      it.className = "w-item";
      it.textContent = String(i);
      track.append(it);
      this.items.push(it);
    }
    const u = document.createElement("div");
    u.className = "w-unit";
    u.textContent = unit;
    this.el.append(track, u);
    this.bind();
    this.draw();
  }

  get(): number {
    return this.mod(Math.round(this.value));
  }

  private mod(v: number) {
    return ((v % this.n) + this.n) % this.n;
  }

  /** kuerzester Abstand im Kreis */
  private dist(i: number) {
    let d = this.mod(i - this.value);
    if (d > this.n / 2) d -= this.n;
    return d;
  }

  private draw() {
    for (let i = 0; i < this.n; i++) {
      const d = this.dist(i);
      const it = this.items[i];
      if (Math.abs(d) > SHOW) { it.style.visibility = "hidden"; continue; }
      it.style.visibility = "";
      const a = Math.abs(d);
      it.style.transform = `translateY(${d * ROW}px) rotateX(${-d * 17}deg) scale(${1 - a * 0.05})`;
      it.style.opacity = String(Math.max(0, 1 - a * 0.3));
      it.classList.toggle("on", a < 0.5);
    }
  }

  private to(target: number, ms = 220) {
    cancelAnimationFrame(this.anim);
    const from = this.value;
    const t0 = performance.now();
    const step = (t: number) => {
      const k = Math.min(1, (t - t0) / ms);
      const e = 1 - Math.pow(1 - k, 3);
      this.value = from + (target - from) * e;
      this.draw();
      if (k < 1) this.anim = requestAnimationFrame(step);
      else { this.value = this.mod(Math.round(target)); this.draw(); }
    };
    this.anim = requestAnimationFrame(step);
  }

  private bind() {
    // Mausrad: genau ein Schritt pro Raste
    let acc = 0;
    let goal = 0;
    let wheelT = 0;
    this.el.addEventListener("wheel", (e) => {
      e.preventDefault();
      e.stopPropagation();
      acc += e.deltaY;
      if (Math.abs(acc) < 30 && Math.abs(e.deltaY) < 30) return; // Touchpad: sammeln
      const steps = Math.sign(acc) * Math.max(1, Math.round(Math.abs(acc) / 100));
      acc = 0;
      // schnelle Folge von Rasten addiert sich, statt jedes Mal neu bei der Mitte anzufangen
      goal = performance.now() - wheelT < 250 ? goal + steps : Math.round(this.value) + steps;
      wheelT = performance.now();
      this.to(goal, 160);
    }, { passive: false });

    // Ziehen mit Schwung
    this.el.addEventListener("pointerdown", (e) => {
      if (e.button !== 0) return;
      e.stopPropagation();
      cancelAnimationFrame(this.anim);
      this.el.setPointerCapture(e.pointerId);
      this.onHold(true);
      const y0 = e.clientY;
      const v0 = this.value;
      let moved = false;
      let lastY = y0, lastT = performance.now(), vel = 0;
      const move = (ev: PointerEvent) => {
        const dy = ev.clientY - y0;
        if (Math.abs(dy) > 3) moved = true;
        const now = performance.now();
        vel = 0.8 * vel + 0.2 * ((ev.clientY - lastY) / Math.max(1, now - lastT)); // px/ms
        lastY = ev.clientY; lastT = now;
        this.value = v0 - dy / ROW;
        this.draw();
      };
      const up = (ev: PointerEvent) => {
        this.el.removeEventListener("pointermove", move);
        this.el.removeEventListener("pointerup", up);
        this.el.removeEventListener("pointercancel", up);
        this.onHold(false);
        if (!moved) {
          // Klick: auf die angeklickte Zahl springen
          const r = this.el.getBoundingClientRect();
          const off = Math.round((ev.clientY - (r.top + r.height / 2)) / ROW);
          this.to(Math.round(this.value) + off);
          return;
        }
        const fling = performance.now() - lastT < 80 ? -vel * 9 : 0; // Zeilen
        this.to(Math.round(this.value + fling), 260 + Math.min(500, Math.abs(fling) * 40));
      };
      this.el.addEventListener("pointermove", move);
      this.el.addEventListener("pointerup", up);
      this.el.addEventListener("pointercancel", up);
    });
  }
}
