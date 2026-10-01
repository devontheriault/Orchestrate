// PROTOTYPE — the launch-page hero's behaviour. See index.html.

const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
function make(tag, props = {}, ...kids) {
  const e = Object.assign(document.createElement(tag), props);
  e.append(...kids);
  return e;
}

// ---- which variant -------------------------------------------------------

const VARIANTS = [
  ["A", "Playground"],
  ["B", "Picture"],
  ["C", "Story"],
  ["D", "Side by side"],
];
const asked = new URLSearchParams(location.search).get("variant")?.toUpperCase();
const variant = VARIANTS.some(([k]) => k === asked) ? asked : "A";
const main = $(`main[data-variant="${variant}"]`);
main.hidden = false;
document.body.dataset.variant = variant;

{
  const i = VARIANTS.findIndex(([k]) => k === variant);
  $(".proto-switcher .label").textContent = `${variant} · ${VARIANTS[i][1]}`;
  const go = (d) => {
    const [k] = VARIANTS[(i + d + VARIANTS.length) % VARIANTS.length];
    const url = new URL(location.href);
    url.searchParams.set("variant", k);
    location.replace(url);
  };
  for (const b of $$(".proto-switcher button")) b.onclick = () => go(Number(b.dataset.go));
  addEventListener("keydown", (e) => {
    if (e.target.closest?.("input, textarea, select, [contenteditable]")) return;
    if (e.key === "ArrowLeft") go(-1);
    if (e.key === "ArrowRight") go(1);
  });
}

// ---- the wallpaper ---------------------------------------------------------

function setWall(name) {
  document.body.dataset.wall = name;
  localStorage.setItem("hero:wall", name);
  for (const s of $$(".swatch")) s.setAttribute("aria-pressed", String(s.dataset.wall === name));
}
setWall(localStorage.getItem("hero:wall") === "code" ? "code" : localStorage.getItem("hero:wall") === "dusk" ? "dusk" : "aurora");

{
  const code = `<span class="c">// Every agent gets its own worktree and branch.</span>
<span class="k">pub async fn</span> <span class="f">spawn_agent</span>(project: &amp;Project, prompt: <span class="k">String</span>) -&gt; Result&lt;Agent&gt; {
    <span class="k">let</span> id = AgentId::<span class="f">new</span>();
    <span class="k">let</span> branch = <span class="f">format!</span>(<span class="s">"cw/agent-{id}"</span>);
    <span class="k">let</span> worktree = git::<span class="f">add_worktree</span>(&amp;project.path, &amp;branch).<span class="k">await</span>?;
    <span class="k">let</span> turn = Turn::<span class="f">start</span>(&amp;worktree, &amp;prompt).<span class="k">await</span>?;
    <span class="f">Ok</span>(Agent { id, branch, worktree, state: State::Running, turn })
}

<span class="k">export function</span> <span class="f">useTheme</span>() {
  <span class="k">const</span> [theme, setTheme] = <span class="f">useLocalStorage</span>(<span class="s">"theme"</span>, <span class="f">systemTheme</span>());
  <span class="f">useEffect</span>(() =&gt; {
    document.documentElement.dataset.theme = theme;
  }, [theme]);
  <span class="k">return</span> [theme, setTheme] <span class="k">as const</span>;
}

<span class="c">/// 30s, 1m, 2m, 4m, 8m.</span>
<span class="k">fn</span> <span class="f">backoff</span>(attempt: <span class="k">u32</span>) -&gt; Duration {
    Duration::<span class="f">from_secs</span>(<span class="s">30</span> &lt;&lt; attempt.<span class="f">min</span>(<span class="s">4</span>))
}

$ npm test -- header theme
 ✓ src/components/Header.test.tsx (9)
 ✓ src/hooks/useTheme.test.ts (5)
      Tests  14 passed (14)

$ git merge --no-ff cw/agent-e40c5b19
Merge made by the 'ort' strategy.
 src/pdf.rs             | 18 ++++++++++++++++++
 src/routes/invoices.rs |  2 ++
`;
  $("#wall .code").innerHTML = (code + "\n").repeat(6);
}

// ---- the window ------------------------------------------------------------

window.__GLASS = 22; // the app's veil, read by demo-host.js

function makeWindow({ locked = false } = {}) {
  const frame = make("iframe", { src: "/app/", title: "Orchestrate, running live" });
  const poster = make("img", { className: "poster", src: "still.png", alt: "" });
  const win = make("div", { className: "glass-window" + (locked ? " locked" : "") }, frame, poster);
  // The still shows until the live app has painted: the first frame is never empty.
  frame.addEventListener("load", () => setTimeout(() => win.classList.add("live"), 700));
  return { win, frame };
}

function toast(kind, title, body = "") {
  const t = make("div", { className: "toast" }, make("small", { textContent: kind }), make("b", { textContent: title }));
  if (body) t.append(make("span", { textContent: body }));
  $(".toasts").append(t);
  setTimeout(() => t.remove(), 5500);
}

// Which installer this visitor wants. Phones and tablets get none: there's
// nothing to install there yet. An iPhone's and an iPad's browser both say
// "Mac OS X", so touch is what tells them from a Mac.
const RELEASES = "https://github.com/devontheriault/Orchestrate/releases";
const os = (() => {
  const ua = navigator.userAgent;
  if (/iPhone|iPad|Android/.test(ua) || (/Macintosh/.test(ua) && navigator.maxTouchPoints > 1)) return null;
  return /Windows/.test(ua) ? "Windows" : /Mac OS X|Macintosh/.test(ua) ? "macOS" : "Linux";
})();
// Picked by ending, since the names carry the version. Updater signatures
// (`.AppImage.sig`) don't match.
const INSTALLER = { Linux: [/\.AppImage$/], macOS: [/\.dmg$/], Windows: [/-setup\.exe$/, /\.msi$/] };

// Every Download button goes to the Releases page until the latest release
// answers, then straight to its installer. A draft or pre-release isn't
// "latest", so nothing points at a release before it's published.
const downloadLinks = [$("nav.top .download")];
function downloads(slot) {
  const primary = make("a", { className: "primary", href: RELEASES, textContent: os ? `Download for ${os}` : "Download" });
  downloadLinks.push(primary);
  slot.append(
    primary,
    make("a", { className: "secondary", href: "https://github.com/devontheriault/Orchestrate#install", textContent: "Other platforms" }),
  );
}
for (const slot of $$('[data-slot="download"]', main)) downloads(slot);
if (os) {
  fetch("https://api.github.com/repos/devontheriault/Orchestrate/releases/latest")
    .then((r) => (r.ok ? r.json() : null))
    .then((release) => {
      const assets = release?.assets ?? [];
      for (const pattern of INSTALLER[os]) {
        const asset = assets.find((a) => pattern.test(a.name));
        if (!asset) continue;
        for (const a of downloadLinks) {
          a.href = asset.browser_download_url;
          a.title = `${asset.name} · ${Math.round(asset.size / 1e6)} MB`;
        }
        return;
      }
    })
    .catch(() => {}); // offline or rate-limited: the Releases page it is
}

// What the app tells the page: window buttons, drags, notifications.
const handlers = {};
addEventListener("message", (e) => {
  const m = e.data;
  if (!m?.demo) return;
  handlers[m.demo]?.(m);
  if (variant === "D") return; // no toasts: the page stays still beside the app
  if (m.demo === "notification") toast("Orchestrate", m.title, m.body);
  if (m.demo === "merged") toast("Merged", `${m.title ?? "Agent"} → ${m.target}`, "Merged, keeping it one commit.");
});

const appDoc = (frame) => frame.contentDocument;
function setVeil(frame, pct) {
  localStorage.setItem("orchestrate:glass-opacity", String(pct));
  appDoc(frame)?.documentElement.style.setProperty("--glass-opacity", pct + "%");
}
function setTheme(frame, name) {
  localStorage.setItem("orchestrate:theme", name);
  const d = appDoc(frame);
  if (d) d.documentElement.dataset.theme = name;
}

// ---- A: Playground ---------------------------------------------------------

if (variant === "A") {
  const stage = $('[data-slot="window"]', main);
  const { win, frame } = makeWindow();
  stage.append(win);

  const size = () => {
    const w = Math.min(1240, innerWidth - 64);
    const h = Math.min(780, Math.max(540, innerHeight - 150));
    stage.style.setProperty("--win-w", w + "px");
    stage.style.setProperty("--win-h", h + "px");
    return { w, h };
  };
  let pos;
  const center = () => {
    const { w } = size();
    pos = { x: (stage.clientWidth - w) / 2, y: 0 };
    place();
  };
  const place = () => {
    win.style.left = pos.x + "px";
    win.style.top = pos.y + "px";
  };
  center();
  addEventListener("resize", () => !moved && center());

  let moved = false;
  handlers.drag = ({ dx, dy }) => {
    moved = true;
    win.classList.add("dragging");
    pos.x += dx;
    pos.y = Math.max(-stage.offsetTop + 70, pos.y + dy);
    place();
  };
  handlers.dragend = () => win.classList.remove("dragging");
  const setMax = (on) => {
    win.classList.toggle("max", on);
    frame.contentWindow.demo && (frame.contentWindow.demo.maximized = on);
  };
  handlers.maximize = () => setMax(!win.classList.contains("max"));
  const relaunch = $(".relaunch");
  handlers.minimize = handlers.close = () => {
    setMax(false);
    win.classList.add("gone");
    relaunch.hidden = false;
  };
  relaunch.onclick = () => {
    win.classList.remove("gone");
    relaunch.hidden = true;
  };

  // The dock: what's behind the glass, how much the glass lets through.
  const dock = $('[data-slot="dock"]', main);
  const file = make("input", { type: "file", accept: "image/*", hidden: true });
  file.onchange = () => {
    const f = file.files?.[0];
    if (!f) return;
    $("#wall .photo").style.setProperty("--photo", `url(${URL.createObjectURL(f)})`);
    setWall("photo");
  };
  const swatch = (wall, title, onclick = () => setWall(wall), text = "") => {
    const b = make("button", { className: `swatch ${wall}-sw`, title, onclick }, text);
    b.dataset.wall = wall;
    return b;
  };
  const swatches = [
    swatch("aurora", "Aurora"),
    swatch("dusk", "Dusk"),
    swatch("code", "Code"),
    swatch("photo", "Your own picture", () => file.click(), "+"),
  ];

  const veil = make("input", { type: "range", min: 0, max: 85, value: 22, title: "How much the glass darkens what's behind it" });
  veil.oninput = () => setVeil(frame, veil.value);

  const theme = make("select", { title: "Theme" });
  for (const [v, label] of [
    ["glass", "Glass"],
    ["tokyo-night", "Tokyo Night"],
    ["catppuccin-mocha", "Catppuccin Mocha"],
    ["rose-pine", "Rosé Pine"],
    ["nord", "Nord"],
    ["gruvbox-dark", "Gruvbox Dark"],
    ["dracula", "Dracula"],
    ["catppuccin-latte", "Catppuccin Latte"],
    ["orchestrate-light", "Orchestrate Light"],
  ])
    theme.append(make("option", { value: v, textContent: label }));
  theme.onchange = () => {
    setTheme(frame, theme.value);
    veilGroup.style.opacity = theme.value === "glass" ? "1" : "0.35";
  };

  const reset = make("button", { className: "plain", textContent: "Reset window", onclick: () => {
    moved = false;
    setMax(false);
    win.classList.remove("gone");
    relaunch.hidden = true;
    center();
  } });

  const veilGroup = make("label", { className: "group" }, "Glass", veil);
  dock.append(
    make("div", { className: "group" }, "Behind", ...swatches),
    file,
    make("span", { className: "sep" }),
    veilGroup,
    make("span", { className: "sep" }),
    make("div", { className: "group" }, "Theme", theme),
    make("span", { className: "sep" }),
    reset,
  );
  setWall(document.body.dataset.wall);
}

// ---- B: Picture ------------------------------------------------------------

if (variant === "B") {
  const tilt = $(".still .tilt", main);
  main.addEventListener("mousemove", (e) => {
    const x = e.clientX / innerWidth - 0.5;
    const y = e.clientY / innerHeight - 0.5;
    tilt.style.transform = `rotateY(${-16 + x * 10}deg) rotateX(${5 - y * 6}deg)`;
  });
}

// ---- C: Story --------------------------------------------------------------

if (variant === "C") {
  const { win, frame } = makeWindow({ locked: true });
  $('[data-slot="window"]', main).append(win);
  const wheel = $(".take-wheel", main);
  const unlock = () => {
    win.classList.remove("locked");
    wheel.hidden = true;
  };
  wheel.onclick = unlock;

  // Driving the app the way a person would: through its own controls.
  const D = () => appDoc(frame);
  const find = (sel, text) => [...D().querySelectorAll(sel)].find((e) => e.textContent.includes(text));
  const click = (e) => e?.dispatchEvent(new (frame.contentWindow.MouseEvent)("click", { bubbles: true }));
  async function expand(project) {
    const row = find("button.row", project);
    if (row?.getAttribute("aria-expanded") === "false") click(row);
    await sleep(250);
  }
  async function select(project, title) {
    await expand(project);
    let a = find("div.agent", title);
    if (!a) {
      for (const f of D().querySelectorAll("button.fold")) if (f.getAttribute("aria-expanded") === "false") click(f);
      await sleep(250);
      a = find("div.agent", title);
    }
    if (a && !a.classList.contains("selected")) click(a);
    await sleep(400);
    return a;
  }
  const tab = async (name) => {
    click(find("[role=tab]", name));
    await sleep(500);
  };
  async function type(text, live) {
    const ta = D().querySelector("textarea");
    if (!ta) return;
    ta.focus();
    const set = Object.getOwnPropertyDescriptor(frame.contentWindow.HTMLTextAreaElement.prototype, "value").set;
    for (let i = 1; i <= text.length; i++) {
      if (!live()) return;
      set.call(ta, text.slice(0, i));
      ta.dispatchEvent(new (frame.contentWindow.InputEvent)("input", { bubbles: true }));
      await sleep(38 + Math.random() * 40);
    }
    await sleep(350);
    ta.dispatchEvent(new (frame.contentWindow.KeyboardEvent)("keydown", { key: "Enter", bubbles: true, cancelable: true }));
  }

  const STEPS = {
    fleet: { hue: "0deg", run: async () => {
      for (const p of ["storefront", "billing-api", "mobile-app"]) await expand(p);
      await select("storefront", "Dark mode toggle");
      await tab("Output");
    } },
    work: { hue: "40deg", run: async () => {
      await select("storefront", "Lazy-load product images");
      await tab("Output");
    } },
    queue: { hue: "80deg", run: async (live) => {
      const busy = D().querySelector("div.agent.running");
      if (busy) click(busy);
      else await select("storefront", "Dark mode toggle");
      await tab("Output");
      await sleep(600);
      await type("Also add a test that the setting survives a reload.", live);
    } },
    diff: { hue: "140deg", run: async () => {
      await select("billing-api", "Invoice PDF export");
      await tab("Diff");
    } },
    merge: { hue: "200deg", run: async () => {
      await select("billing-api", "Invoice PDF export");
      await tab("Diff");
      await sleep(900);
      click([...D().querySelectorAll("button.btn-primary")].find((b) => b.textContent.trim() === "Merge"));
    } },
    yours: { hue: "280deg", run: async () => unlock() },
  };

  let ready = new Promise((r) => frame.addEventListener("load", () => setTimeout(r, 1500)));
  let current = null;
  let token = 0;
  const done = new Set();
  async function enter(name) {
    if (current === name) return;
    current = name;
    for (const s of $$(".step", main)) s.classList.toggle("active", s.dataset.step === name);
    document.body.style.setProperty("--hue", STEPS[name].hue);
    if (name !== "yours" && !win.classList.contains("locked")) return; // the visitor is driving
    const mine = ++token;
    await ready;
    if (mine !== token) return;
    // Typing a follow-up and merging only happen once; the rest replay.
    if ((name === "queue" || name === "merge") && done.has(name)) return;
    done.add(name);
    await STEPS[name].run(() => mine === token);
  }
  wheel.hidden = false;
  const io = new IntersectionObserver(
    (entries) => {
      for (const e of entries) if (e.isIntersecting) enter(e.target.dataset.step);
    },
    { rootMargin: "-45% 0px -45% 0px" },
  );
  for (const s of $$(".step", main)) io.observe(s);
  enter("fleet");
}

// ---- D: Side by side -------------------------------------------------------

if (variant === "D") {
  // The window is the visitor's from the first frame. Scrolling only changes
  // the page around it; it never clicks anything in the app.
  const { win, frame } = makeWindow();
  $('[data-slot="window"]', main).append(win);

  // No minimise, maximise or close: the window stays where it is. With no
  // handlers here, a double-click on the title bar does nothing either. No
  // Settings: its menu covers the app and has nothing to show off here.
  frame.addEventListener("load", () => {
    const style = appDoc(frame).createElement("style");
    style.textContent = ".chrome .controls, aside footer:has(button.settings) { display: none !important; }";
    appDoc(frame).head.append(style);
  });

  const HUES = { fleet: "0deg", work: "40deg", queue: "80deg", diff: "140deg", merge: "200deg", yours: "280deg" };
  const enter = (name) => {
    for (const s of $$(".step", main)) s.classList.toggle("active", s.dataset.step === name);
    document.body.style.setProperty("--hue", HUES[name]);
  };
  const io = new IntersectionObserver(
    (entries) => {
      for (const e of entries) if (e.isIntersecting) enter(e.target.dataset.step);
    },
    { rootMargin: "-45% 0px -45% 0px" },
  );
  for (const s of $$(".step", main)) io.observe(s);
  enter("fleet");
}
