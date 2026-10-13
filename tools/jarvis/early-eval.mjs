// Jarvis M2c: die Vorab-Entscheidung (src/jarvis-live.ts, Klasse Turn) mit echten Teiltexten pruefen.
// Jeder Satz aus saetze.json kommt Wort fuer Wort an (wie die Teiltexte der Mitschrift); je Teiltext:
// Korrekturwoerter -> Programmname (hier vereinfacht nachgebaut, in der Notch jarvis.rs mention) -> Laya -> Turn.
//   LAYA_TS=D:/Dev/_downloads/laya-build/laya/laya-ts/dist/index.js node --experimental-transform-types tools/jarvis/early-eval.mjs <modellordner>
import fs from "node:fs";
import { pathToFileURL } from "node:url";

const { Agent } = await import(pathToFileURL(process.env.LAYA_TS ?? "").href);
const { Turn, KORREKTUR, SURE, MIN_P } = await import(new URL("../../src/jarvis-live.ts", import.meta.url).href);
const { saetze } = JSON.parse(fs.readFileSync(new URL("./saetze.json", import.meta.url), "utf8"));
const worker = fs.readFileSync(new URL("../../src/laya/worker.ts", import.meta.url), "utf8");
// dieselbe Frage wie im Worker (dort steht der trainierte Wortlaut)
const ABSICHT = Function(`return ${/const ABSICHT = (\{[\s\S]*?\n\});/.exec(worker)[1]}`)();

// gesprochene Namen -> Programm (wie jarvis.rs: ganzes Wort, Allerweltswoerter wie „Editor“ zaehlen nicht)
const NAMES = { chrome: "chrome", "google chrome": "chrome", firefox: "firefox", spotify: "spotify", discord: "discord",
  steam: "steam", league: "league", "league of legends": "league", explorer: "explorer", haze: "haze", epic: "epic" };
const norm = (s) => s.toLowerCase().replace(/[^\p{L}\p{N}]+/gu, " ").trim();
function mention(text) {
  const t = ` ${norm(text)} `;
  let best = null;
  for (const [n, app] of Object.entries(NAMES)) {
    const pos = t.lastIndexOf(` ${n} `);
    if (pos >= 0 && (!best || pos > best.pos || (pos === best.pos && n.length > best.len))) best = { pos, len: n.length, app };
  }
  return best?.app ?? null;
}

for (const dir of process.argv.slice(2)) {
  const agent = await Agent.load(dir, { numThreads: Number(process.env.T ?? 8) });
  let soll = 0, spaet = 0, richtig = 0, falsch = 0, frueh = 0, undo = 0, laeufe = 0;
  const zeilen = [];
  for (const s of saetze) {
    const should = s.absicht === "programm" || s.absicht === "fenster" ? (s.fertig && !s.zurueck && s.app !== "keins" ? s.app : null) : null;
    if (should) soll++;
    const turn = new Turn();
    const words = s.t.split(" ");
    let fired = null, firedAt = -1, undone = false;
    // + 1: am Satzende schickt die Mitschrift den fertigen Text noch einmal (…completed)
    for (let i = 1; i <= words.length + 1; i++) {
      const text = words.slice(0, i).join(" ");
      const k = turn.korrektur(text);
      if (k.kind === "undo") { undone = true; continue; }
      if (turn.closed) continue;
      const app = mention(text);
      if (!app) { turn.laya(null, "nichts", 0); continue; }
      laeufe++;
      const a = (await agent.predict(text, ABSICHT)).answers.absicht;
      const step = turn.laya(app, a.choice, a.probabilities[a.choice] ?? 0);
      if (step.kind !== "none") { fired = `${step.kind}:${step.app}`; firedAt = i; }
    }
    // gilt: richtig gestartet und nicht zurueckgenommen; zurueckgenommen zaehlt als Fehlstart, der korrigiert wurde
    const app = fired?.split(":")[1];
    const ok = fired && app === should && !undone;
    if (ok) { richtig++; if (firedAt < words.length) frueh++; else if (firedAt > words.length) spaet++; }
    else if (fired && !undone) falsch++;
    if (undone) undo++;
    zeilen.push(`${ok ? "✓" : fired && !undone ? "✗" : undone ? "↺" : should ? "·" : " "} ${(fired ?? "-").padEnd(18)} ${firedAt > 0 ? `@${firedAt}/${words.length}` : "     "} | ${s.t}`);
  }
  console.log(`\n== ${dir}  (sofort ab ${SURE}, sonst zweimal ab ${MIN_P}; Korrektur: ${KORREKTUR.source.slice(0, 40)}…)`);
  if (process.env.V) console.log(zeilen.join("\n"));
  console.log(`vorab richtig ${richtig}/${soll} (davon ${frueh} vor dem letzten Wort, ${spaet} erst mit dem Endtext) · Fehlstarts ${falsch} · zurückgenommen ${undo} · Laya-Läufe ${laeufe}`);
}
