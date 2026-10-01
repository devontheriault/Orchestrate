// Builds the website into site/dist/, ready to publish as static files.
//
//   npm run site:build                 build the app, then assemble the site
//   npm run site:build -- --no-app     reuse the app's last build (build/)
//
// /             the landing page (this folder)
// /app/         the app's own static build, with demo-host.js put in front of
//               everything so it has a Host to talk to. SvelteKit's built page
//               works out its base from `location`, so it runs under /app/
//               without touching svelte.config.js.
// /fonts/       the two typefaces the page uses, from node_modules
// /logo-dark.svg
// /favicon.svg  the app icon, dark in a light browser so it shows on a pale tab
import { execSync } from "node:child_process";
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const here = fileURLToPath(new URL(".", import.meta.url));
const root = join(here, "..");
const dist = join(here, "dist");

if (!process.argv.includes("--no-app")) {
  console.log("Building the app…");
  execSync("npx vite build", { cwd: root, stdio: ["ignore", "ignore", "inherit"] });
}

rmSync(dist, { recursive: true, force: true });
mkdirSync(dist);
for (const f of ["index.html", "hero.css", "hero.js", "demo-host.js"]) cpSync(join(here, f), join(dist, f));

cpSync(join(root, "build"), join(dist, "app"), { recursive: true });
const appPage = join(dist, "app/index.html");
writeFileSync(appPage, readFileSync(appPage, "utf8").replace("<head>", '<head><script src="/demo-host.js"></script>'));

for (const font of ["geist", "jetbrains-mono"]) {
  const from = join(root, "node_modules/@fontsource-variable", font);
  cpSync(join(from, "index.css"), join(dist, "fonts", font, "index.css"));
  cpSync(join(from, "files"), join(dist, "fonts", font, "files"), { recursive: true });
}
cpSync(join(root, "docs/assets/logo-dark.svg"), join(dist, "logo-dark.svg"));
const icon = readFileSync(join(root, "src-tauri/icons/icon.svg"), "utf8");
writeFileSync(
  join(dist, "favicon.svg"),
  icon.replace(/(<svg[^>]*>)/, "$1<style>@media (prefers-color-scheme: light) { svg { fill: #111 } }</style>"),
);

console.log("Built site/dist/");
