// PROTOTYPE — captures still.png, the Picture variant's image and the poster
// the window shows until the live app has loaded: the app at /app/ with the
// page left see-through, as the glass needs. Run with the server up:
//   node site/prototype-hero/capture-still.mjs
import { spawn } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const port = 9351;
const chrome = spawn(
  "chromium",
  ["--headless=new", `--remote-debugging-port=${port}`, `--user-data-dir=${mkdtempSync(join(tmpdir(), "still-"))}`, "--hide-scrollbars", "--force-color-profile=srgb", "about:blank"],
  { stdio: "ignore" },
);
let targets = [];
for (let i = 0; i < 50 && !targets.length; i++) {
  await sleep(200);
  targets = await fetch(`http://127.0.0.1:${port}/json/list`).then((r) => r.json(), () => []);
}
const ws = new WebSocket(targets.find((t) => t.type === "page").webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener("open", r, { once: true }));
let id = 0;
const pending = new Map();
ws.addEventListener("message", (m) => {
  const msg = JSON.parse(m.data);
  pending.get(msg.id)?.(msg.result);
});
const send = (method, params = {}) => new Promise((r) => (pending.set(++id, r), ws.send(JSON.stringify({ id, method, params }))));

await send("Emulation.setDeviceMetricsOverride", { width: 1240, height: 780, deviceScaleFactor: 2, mobile: false });
await send("Emulation.setDefaultBackgroundColorOverride", { color: { r: 0, g: 0, b: 0, a: 0 } });
await send("Page.navigate", { url: `http://127.0.0.1:${process.env.PORT ?? 4317}/app/` });
await sleep(9000); // a few live steps in, so the running agents say what they're doing
const { data } = await send("Page.captureScreenshot", { format: "png" });
const out = new URL("./still.png", import.meta.url);
writeFileSync(out, Buffer.from(data, "base64"));
console.log("wrote", out.pathname);
ws.close();
chrome.kill();
