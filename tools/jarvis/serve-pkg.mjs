// Statischer Server fuer Tests des Modellpakets: wie ein GitHub-Release (Range fuer fortgesetzte Downloads,
// laya.rs-Test laya_download) und mit CORS, damit tools/jarvis/worker-test.html (Vite) die Dateien laden kann.
//   node tools/jarvis/serve-pkg.mjs <ordner> [port=8767]
import http from "node:http"; import fs from "node:fs"; import path from "node:path";
const root = path.resolve(process.argv[2] ?? ".");
const port = Number(process.argv[3] ?? 8767);
http.createServer((req, res) => {
  const f = path.join(root, decodeURIComponent(req.url.split("?")[0]));
  if (!f.startsWith(root)) { res.writeHead(403); return res.end(); }
  fs.stat(f, (e, st) => {
    const cors = { "Access-Control-Allow-Origin": "*" };
    if (e || !st.isFile()) { res.writeHead(404, cors); return res.end(); }
    const m = /bytes=(\d+)-/.exec(req.headers.range ?? "");
    if (m) {
      const start = Number(m[1]);
      res.writeHead(206, { ...cors, "Content-Length": st.size - start, "Content-Range": `bytes ${start}-${st.size - 1}/${st.size}` });
      return fs.createReadStream(f, { start }).pipe(res);
    }
    res.writeHead(200, { ...cors, "Content-Length": st.size });
    fs.createReadStream(f).pipe(res);
  });
}).listen(port, "127.0.0.1", () => console.log(`${root} auf http://127.0.0.1:${port}`));
