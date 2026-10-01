// PROTOTYPE — the launch-page hero's behaviour. See index.html.

const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];

const ua = navigator.userAgent;
// An iPad's browser says "Mac OS X" like a Mac does, so touch tells them apart.
const ios = /iPhone|iPad/.test(ua) || (/Macintosh/.test(ua) && navigator.maxTouchPoints > 1);

// iOS zooms the page in when a text box under 16px takes focus, which the
// app's message box is. `maximum-scale=1` stops that, and iOS still lets you
// pinch-zoom. Only there: Android doesn't zoom on focus, and would honour it by
// blocking pinch-zoom. The app's own page does the same (app.html), but inside
// the window only this page's viewport counts.
if (ios) $('meta[name="viewport"]').content += ", maximum-scale=1";

// ---- the window ------------------------------------------------------------

const win = $(".glass-window");
const frame = $("iframe", win);
const app = () => frame.contentDocument;

// The iframe can finish loading before this module runs.
function whenLoaded(fn) {
  if (app()?.readyState === "complete" && app().location.pathname.startsWith("/app")) fn();
  else frame.addEventListener("load", fn, { once: true });
}

whenLoaded(() => {
  // No minimise, maximise or close: the window stays where it is. No Settings:
  // its menu covers the app and has nothing to show off here.
  const style = app().createElement("style");
  style.textContent = ".chrome .controls, aside footer:has(button.settings) { display: none !important; }";
  app().head.append(style);

  // The app fades in once it has drawn its bar and sidebar, not on `load`:
  // the page loads before the app has rendered anything. Until then the
  // window is empty glass. If the app never draws them, show it anyway.
  const drawn = () => app()?.querySelector("header.chrome") && app().querySelector("aside");
  const wait = () => (drawn() ? requestAnimationFrame(() => win.classList.add("live")) : requestAnimationFrame(wait));
  wait();
  setTimeout(() => win.classList.add("live"), 5000);
});

// ---- the story -------------------------------------------------------------

// Each step tints the wallpaper as it comes to the middle of the screen.
const HUES = { fleet: "0deg", work: "40deg", queue: "80deg", diff: "140deg", merge: "200deg", yours: "280deg" };
function enter(name) {
  for (const s of $$(".step")) s.classList.toggle("active", s.dataset.step === name);
  document.body.style.setProperty("--hue", HUES[name]);
}
const io = new IntersectionObserver(
  (entries) => {
    for (const e of entries) if (e.isIntersecting) enter(e.target.dataset.step);
  },
  { rootMargin: "-45% 0px -45% 0px" },
);
for (const s of $$(".step")) io.observe(s);
enter("fleet");

// ---- downloads -------------------------------------------------------------

// Which installer this visitor wants. Phones and tablets get none: there's
// nothing to install there yet.
const os = ios || /Android/.test(ua) ? null : /Windows/.test(ua) ? "Windows" : /Mac OS X|Macintosh/.test(ua) ? "macOS" : "Linux";
// Picked by ending, since the names carry the version. Updater signatures
// (`.AppImage.sig`) don't match.
const INSTALLER = { Linux: [/\.AppImage$/], macOS: [/\.dmg$/], Windows: [/-setup\.exe$/, /\.msi$/] };

// Every Download button goes to the Releases page until the latest release
// answers, then straight to its installer. A draft or pre-release isn't
// "latest", so nothing points at a release before it's published.
if (os) {
  $(".ctas .download").textContent = `Download for ${os}`;
  fetch("https://api.github.com/repos/devontheriault/Orchestrate/releases/latest")
    .then((r) => (r.ok ? r.json() : null))
    .then((release) => {
      const assets = release?.assets ?? [];
      for (const pattern of INSTALLER[os]) {
        const asset = assets.find((a) => pattern.test(a.name));
        if (!asset) continue;
        for (const a of $$("a.download")) {
          a.href = asset.browser_download_url;
          a.title = `${asset.name} · ${Math.round(asset.size / 1e6)} MB`;
        }
        return;
      }
    })
    .catch(() => {}); // offline or rate-limited: the Releases page it is
}
