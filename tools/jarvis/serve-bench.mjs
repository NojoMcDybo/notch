// statischer Server fuer die Messseite (COOP/COEP fuer WASM-Threads)
import http from "node:http"; import fs from "node:fs"; import path from "node:path";
// Wurzel: Laya-Build-Ordner mit laya/laya-ts/node_modules/onnxruntime-web und den Modellordnern
const root = path.resolve(process.argv[2] ?? ".");
const page = path.join(import.meta.dirname, "webgpu.html");
const types = { ".html": "text/html", ".mjs": "text/javascript", ".js": "text/javascript", ".wasm": "application/wasm" };
http.createServer((req, res) => {
  const u = decodeURIComponent(req.url.split("?")[0]);
  const f = u === "/" ? page : path.join(root, u);
  if (f !== page && !f.startsWith(root)) { res.writeHead(403); return res.end(); }
  fs.stat(f, (e, st) => {
    if (e || !st.isFile()) { res.writeHead(404); return res.end(); }
    res.writeHead(200, { "Content-Type": types[path.extname(f)] ?? "application/octet-stream", "Content-Length": st.size,
      "Cross-Origin-Opener-Policy": "same-origin", "Cross-Origin-Embedder-Policy": "require-corp" });
    fs.createReadStream(f).pipe(res);
  });
}).listen(8770, "127.0.0.1", () => console.log("http://127.0.0.1:8770"));
