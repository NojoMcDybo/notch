// Jarvis-Frueherkenner messen (docs/jarvis.md, M2a): die vier Fragen an Laya ueber saetze.json.
// Braucht laya-ts (gebaut) und ein exportiertes Modell, z. B. aus D:\Dev\_downloads\laya-build:
//   LAYA_TS=D:/Dev/_downloads/laya-build/laya/laya-ts/dist/index.js node tools/jarvis/eval.mjs D:/Dev/_downloads/laya-build/model-ml [weitere Modellordner]
import fs from "node:fs";
import { pathToFileURL } from "node:url";

const { Agent } = await import(pathToFileURL(process.env.LAYA_TS ?? "").href);
const { saetze } = JSON.parse(fs.readFileSync(new URL("./saetze.json", import.meta.url), "utf8"));

export const FRAGEN = {
  absicht: { type: "choice", instructions: "Was will der Sprecher gerade vom PC?", criteria: {
    programm: "ein Programm öffnen oder starten",
    fenster: "ein schon offenes Fenster nach vorn holen oder dorthin wechseln",
    seite: "eine Webseite öffnen oder im Internet suchen",
    sonstiges: "etwas anderes, eine Frage, Musik, Timer, oder noch unklar",
  } },
  app: { type: "choice", instructions: "Welches Programm ist gemeint?", criteria: {
    chrome: "Google Chrome", firefox: "Firefox", spotify: "Spotify", discord: "Discord", steam: "Steam",
    league: "League of Legends", explorer: "Explorer", editor: "Editor", haze: "Haze", keins: "keins davon",
  } },
  fertig: { type: "noul", instructions: "Ist der Name des Programms vollständig ausgesprochen?" },
  zurueck: { type: "noul", instructions: "Nimmt der Sprecher etwas zurück, verneint er oder korrigiert er sich?" },
};

const pct = (n, d) => `${n}/${d} (${Math.round((100 * n) / d)} %)`;
for (const dir of process.argv.slice(2)) {
  const t0 = performance.now();
  const agent = await Agent.load(dir, { numThreads: Number(process.env.T ?? 8) });
  const load = performance.now() - t0;
  // ein Durchlauf je Satz, wie spaeter je Teiltext
  const times = [];
  const out = [];
  for (const s of saetze) {
    const t = performance.now();
    out.push((await agent.predict(s.t, FRAGEN)).answers);
    times.push(performance.now() - t);
  }
  times.sort((a, b) => a - b);
  const hit = { absicht: 0, app: 0, fertig: 0, zurueck: 0 };
  const rows = [];
  saetze.forEach((s, i) => {
    const a = out[i];
    const ok = {
      absicht: a.absicht.choice === s.absicht,
      app: a.app.choice === s.app,
      fertig: a.fertig.noul >= 0.5 === s.fertig,
      zurueck: a.zurueck.noul >= 0.5 === s.zurueck,
    };
    for (const k in ok) hit[k] += ok[k] ? 1 : 0;
    rows.push(`${Object.values(ok).map((v) => (v ? "·" : "✗")).join("")} ${a.absicht.choice.padEnd(9)} ${a.app.choice.padEnd(8)} ${(a.app.answer_confidence ?? 0).toFixed(2)} f${a.fertig.noul.toFixed(2)} z${a.zurueck.noul.toFixed(2)} | ${s.t}`);
  });
  // Was zaehlt: wuerde der Frueherkenner falsch starten? (absicht=programm, App erkannt, fertig, nicht zurueck)
  const fire = (a) => a.absicht.choice === "programm" && a.app.choice !== "keins" && a.fertig.noul >= 0.5 && a.zurueck.noul < 0.5;
  const shouldFire = (s) => s.absicht === "programm" && s.app !== "keins" && s.fertig && !s.zurueck;
  let falsch = 0, richtig = 0, soll = 0;
  saetze.forEach((s, i) => {
    if (shouldFire(s)) soll++;
    if (fire(out[i])) (shouldFire(s) && out[i].app.choice === s.app ? richtig++ : falsch++);
  });
  console.log(`\n== ${dir}\nladen ${load.toFixed(0)} ms · je Satz p50 ${times[times.length >> 1].toFixed(0)} ms, p90 ${times[Math.floor(times.length * 0.9)].toFixed(0)} ms`);
  for (const k in hit) console.log(`${k.padEnd(8)} ${pct(hit[k], saetze.length)}`);
  console.log(`Start richtig ${pct(richtig, soll)} · Start falsch ${falsch}`);
  if (process.env.V) console.log(rows.join("\n"));
}
