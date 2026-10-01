// Builds the website (build.mjs) and serves site/dist/ to preview it, exactly
// as it will be published.
//
//   npm run site                       build the app and the site, then serve on :4317
//   npm run site -- --no-app           reuse the app's last build: for changes to the page only
//   HOST=$(tailscale ip -4) npm run site   reachable over the tailnet
import { createServer } from "node:http";
import { readFile, stat } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const here = fileURLToPath(new URL(".", import.meta.url));
const dist = join(here, "dist");
const port = Number(process.env.PORT ?? 4317);
const host = process.env.HOST ?? "127.0.0.1";

execFileSync(process.execPath, [join(here, "build.mjs"), ...process.argv.slice(2)], { stdio: "inherit" });

const types = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript",
  ".css": "text/css",
  ".json": "application/json",
  ".png": "image/png",
  ".svg": "image/svg+xml",
  ".woff2": "font/woff2",
};

createServer(async (req, res) => {
  let file = join(dist, normalize(decodeURIComponent(new URL(req.url, "http://x").pathname)));
  try {
    // A folder serves its index.html, as GitHub Pages does.
    if ((await stat(file)).isDirectory()) {
      if (!req.url.endsWith("/")) return void res.writeHead(301, { location: req.url + "/" }).end();
      file = join(file, "index.html");
    }
    const body = await readFile(file);
    res.writeHead(200, { "content-type": types[extname(file)] ?? "application/octet-stream", "cache-control": "no-store" });
    res.end(body);
  } catch {
    res.writeHead(404).end("not found");
  }
}).listen(port, host, () => {
  console.log(`\n  Site: http://${host}:${port}/\n`);
});
