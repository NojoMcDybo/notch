// Erzeugt src-tauri/vcam/src/logo.bin aus src-tauri/icons/source.png: der weisse Notch-Schriftzug als Graustufenmaske
// (u16 Breite, u16 Hoehe, little endian, dann Breite*Hoehe Byte Helligkeit 0..255). Die virtuelle Kamera zeigt ihn
// mittig auf Schwarz, wenn kein Bild vom iPhone kommt.
//
//   node tools/vcam-logo.mjs [Zielhoehe in Pixeln, Standard 260]
//
// Nur noetig, wenn sich das Logo aendert; das Ergebnis liegt im Repo.
import { readFileSync, writeFileSync } from "node:fs";
import { inflateSync } from "node:zlib";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const height = Number(process.argv[2] || 260);

function decodePng(buf) {
  if (buf.readUInt32BE(0) !== 0x89504e47) throw new Error("kein PNG");
  let p = 8, w = 0, h = 0, depth = 0, type = 0, interlace = 0;
  const idat = [];
  while (p < buf.length) {
    const len = buf.readUInt32BE(p), kind = buf.toString("ascii", p + 4, p + 8), d = buf.subarray(p + 8, p + 8 + len);
    if (kind === "IHDR") [w, h, depth, type, interlace] = [d.readUInt32BE(0), d.readUInt32BE(4), d[8], d[9], d[12]];
    else if (kind === "IDAT") idat.push(d);
    else if (kind === "IEND") break;
    p += 12 + len;
  }
  const bpp = { 0: 1, 2: 3, 4: 2, 6: 4 }[type];
  if (depth !== 8 || !bpp || interlace) throw new Error(`PNG-Format nicht unterstuetzt (Tiefe ${depth}, Typ ${type})`);
  const raw = inflateSync(Buffer.concat(idat)), stride = w * bpp, out = Buffer.alloc(h * stride);
  for (let y = 0; y < h; y++) {
    const f = raw[y * (stride + 1)], src = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    for (let x = 0; x < stride; x++) {
      const a = x >= bpp ? out[y * stride + x - bpp] : 0, b = y ? out[(y - 1) * stride + x] : 0;
      const c = x >= bpp && y ? out[(y - 1) * stride + x - bpp] : 0;
      let v = src[x];
      if (f === 1) v += a;
      else if (f === 2) v += b;
      else if (f === 3) v += (a + b) >> 1;
      else if (f === 4) {
        const pa = Math.abs(b - c), pb = Math.abs(a - c), pc = Math.abs(a + b - 2 * c);
        v += pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
      }
      out[y * stride + x] = v & 255;
    }
  }
  const lum = new Float32Array(w * h);
  for (let i = 0; i < w * h; i++) {
    const o = i * bpp;
    lum[i] = bpp >= 3 ? 0.299 * out[o] + 0.587 * out[o + 1] + 0.114 * out[o + 2] : out[o];
  }
  return { w, h, lum };
}

const { w, h, lum } = decodePng(readFileSync(join(root, "src-tauri/icons/source.png")));
// nur im Inneren der schwarzen Kachel suchen (die Ecken ausserhalb sind weiss)
const m = Math.round(w * 0.15);
let [x0, y0, x1, y1] = [w, h, 0, 0];
for (let y = m; y < h - m; y++)
  for (let x = m; x < w - m; x++)
    if (lum[y * w + x] > 60) [x0, y0, x1, y1] = [Math.min(x0, x), Math.min(y0, y), Math.max(x1, x), Math.max(y1, y)];
if (x1 <= x0) throw new Error("kein Schriftzug gefunden");
const [cw, ch] = [x1 - x0 + 1, y1 - y0 + 1];
const s = height / ch, ow = Math.max(1, Math.round(cw * s)), oh = height;

// Flaechenmittel (Box-Filter): jedes Zielpixel mittelt die Quellpixel, die es ueberdeckt
const res = Buffer.alloc(4 + ow * oh);
res.writeUInt16LE(ow, 0);
res.writeUInt16LE(oh, 2);
for (let oy = 0; oy < oh; oy++) {
  for (let ox = 0; ox < ow; ox++) {
    const sx0 = x0 + ox / s, sx1 = x0 + (ox + 1) / s, sy0 = y0 + oy / s, sy1 = y0 + (oy + 1) / s;
    let sum = 0, area = 0;
    for (let y = Math.floor(sy0); y < Math.ceil(sy1); y++) {
      const fy = Math.min(sy1, y + 1) - Math.max(sy0, y);
      for (let x = Math.floor(sx0); x < Math.ceil(sx1); x++) {
        const fx = Math.min(sx1, x + 1) - Math.max(sx0, x), a = fx * fy;
        sum += (x >= x0 && x <= x1 && y >= y0 && y <= y1 ? lum[y * w + x] : 0) * a;
        area += a;
      }
    }
    // Schwarz der Kachel (leicht ueber 0 durch Kompression) wegschneiden
    const v = Math.max(0, (sum / area - 12) * (255 / 243));
    res[4 + oy * ow + ox] = Math.min(255, Math.round(v));
  }
}
const target = join(root, "src-tauri/vcam/src/logo.bin");
writeFileSync(target, res);
console.log(`Ausschnitt ${cw}x${ch} @ (${x0},${y0}) -> ${ow}x${oh}, ${res.length} Byte -> ${target}`);
