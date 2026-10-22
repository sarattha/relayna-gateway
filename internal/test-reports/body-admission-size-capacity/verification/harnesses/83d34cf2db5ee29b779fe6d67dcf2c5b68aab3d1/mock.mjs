import http from "node:http";
import { createHash } from "node:crypto";

http.createServer((req, res) => {
  if (req.url === "/health") return res.end("ok");
  let bytes = 0;
  const hash = createHash("sha256");
  req.on("data", chunk => { bytes += chunk.length; hash.update(chunk); });
  req.on("error", () => res.destroy());
  req.on("end", () => {
    const result = JSON.stringify({
      bytes, sha256: hash.digest("hex"),
      credential_ok: req.headers.authorization === `Bearer ${process.env.SERVICE_TOKEN}`,
    });
    setTimeout(() => {
      res.writeHead(200, { "content-type": "application/json", "content-length": Buffer.byteLength(result) });
      res.end(result);
    }, req.url.startsWith("/warmup") ? 0 : Number(process.env.DELAY_MS ?? 12000));
  });
  const readDelay = req.url.startsWith("/warmup") ? 0 : Number(process.env.READ_DELAY_MS ?? 0);
  if (readDelay > 0) {
    req.pause();
    setTimeout(() => req.resume(), readDelay);
  }
}).listen(4000, "0.0.0.0");
