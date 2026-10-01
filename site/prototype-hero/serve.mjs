// PROTOTYPE — builds the real app and serves the launch-page hero around it.
//
//   npm run site:prototype            build the app, then serve on :4317
//   npm run site:prototype -- --no-build   serve the last build
//   HOST=$(tailscale ip -4) npm run site:prototype   reachable over the tailnet
//
// /       the landing page (this folder)
// /app/   the app's own static build (`vite build` → build/), with
//         demo-host.js put in front of everything so it has a Host to talk to.
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { execSync } from "node:child_process";
import { extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const here = fileURLToPath(new URL(".", import.meta.url));
const root = join(here, "../..");
const port = Number(process.env.PORT ?? 4317);
const host = process.env.HOST ?? "127.0.0.1";

if (!process.argv.includes("--no-build")) {
  console.log("Building the app…");
  execSync("npx vite build", { cwd: root, stdio: ["ignore", "ignore", "inherit"] });
}

const types = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript",
  ".mjs": "text/javascript",
  ".css": "text/css",
  ".json": "application/json",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".svg": "image/svg+xml",
  ".woff2": "font/woff2",
};

createServer(async (req, res) => {
  const path = normalize(decodeURIComponent(new URL(req.url, "http://x").pathname));
  let file;
  if (path === "/app") {
    res.writeHead(301, { location: "/app/" }).end();
    return;
  } else if (path.startsWith("/app/")) {
    file = join(root, "build", path.slice(5) || "index.html");
  } else if (path.startsWith("/node_modules/@fontsource-variable/") || path.startsWith("/docs/assets/")) {
    file = join(root, path);
  } else {
    file = join(here, path === "/" ? "index.html" : path);
  }
  try {
    let body = await readFile(file);
    if (file === join(root, "build/index.html")) {
      body = body.toString().replace("<head>", '<head><script src="/demo-host.js"></script>');
    }
    res.writeHead(200, { "content-type": types[extname(file)] ?? "application/octet-stream", "cache-control": "no-store" });
    res.end(body);
  } catch {
    res.writeHead(404).end("not found");
  }
}).listen(port, host, () => {
  console.log(`\n  Hero prototype: http://${host}:${port}/\n`);
});
