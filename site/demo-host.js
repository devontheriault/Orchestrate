// A stand-in Host for the real Orchestrate frontend, so the app can
// run inside a web page. Loaded into the app's index.html before anything else
// (build.mjs puts it there). It answers every IPC call from a staged fleet, and
// keeps that fleet alive: running agents play scripted turns, follow-ups the
// visitor sends get answered, Commit and Merge work.
(() => {
  const NOW = Date.now();
  const ago = (s) => new Date(NOW - s * 1000).toISOString();
  const now = () => new Date().toISOString();
  const calls = (window.__calls = []);

  // Every visitor gets the app's own window frame, buttons and all. macOS's
  // frame leaves the buttons to the OS, which isn't here to draw them.
  Object.defineProperty(navigator, "userAgent", { value: "Mozilla/5.0 (X11; Linux x86_64)" });
  localStorage.setItem("orchestrate:theme", "glass");
  localStorage.setItem("orchestrate:glass-opacity", "22");
  localStorage.setItem("orchestrate:panes", JSON.stringify({ projects: 300 }));
  sessionStorage.setItem(
    "cw:resume-selection",
    JSON.stringify({ project: "github.com/acme/storefront", agent: "a1c0d4e2" }),
  );


  const projects = [
    { id: "p1", name: "storefront", path: "/home/dev/code/storefront", added_at: ago(9e5), cloned: false, remote: "git@github.com:acme/storefront.git" },
    { id: "p2", name: "billing-api", path: "/home/dev/code/billing-api", added_at: ago(8e5), cloned: false, remote: "git@github.com:acme/billing-api.git" },
    { id: "p3", name: "mobile-app", path: "/home/dev/code/mobile-app", added_at: ago(7e5), cloned: false, remote: "git@github.com:acme/mobile-app.git" },
  ];

  const agent = (id, project, o) => ({
    id,
    project_id: project,
    task: { prompt: o.prompt },
    state: o.state,
    worktree_path: `/home/dev/.local/state/orchestrate/worktrees/${project}/${id}`,
    branch: `cw/agent-${id}`,
    base_commit: "4f2a9c1",
    session_id: "s-" + id,
    model: o.model,
    effort: null,
    permission_mode: null,
    options: {},
    turns: o.turns ?? 1,
    title: o.title,
    user_title: null,
    color: o.color ?? null,
    spawned_at: ago(o.spawned),
    turn_started_at: ago(o.started ?? o.spawned),
    exited_at: o.state === "running" ? null : ago(o.exited ?? 60),
    exit_code: o.state === "running" ? null : 0,
    fail_reason: null,
    merged_branch: o.merged ? "main" : null,
    merged_at: o.merged ? ago(o.exited ?? 60) : null,
    unpushed: false,
    push_error: null,
    resolves: null,
    queue: o.queue ?? [],
  });

  const OPUS = "claude-opus-5-5";
  const SONNET = "claude-sonnet-5";

  const agents = [
    agent("a1c0d4e2", "p1", { title: "Dark mode toggle in the header", prompt: "Add a dark mode toggle to the header. Remember the choice between visits.", state: "running", model: OPUS, spawned: 262, started: 41, turns: 2 }),
    agent("b7e19f03", "p1", { title: "Lazy-load product images", prompt: "Product grid images should lazy-load, with a blurred placeholder.", state: "running", model: SONNET, spawned: 128 }),
    agent("c2f8a611", "p1", { title: "Unify price formatting", prompt: "We format prices three different ways. Make it one helper.", state: "completed", model: SONNET, spawned: 26000, exited: 21600, merged: true, turns: 2 }),
    agent("d93b27aa", "p2", { title: "Retry webhooks with backoff", prompt: "Failed webhook deliveries should retry with exponential backoff, max 6 attempts.", state: "running", model: OPUS, spawned: 377, started: 377 }),
    agent("e40c5b19", "p2", { title: "Invoice PDF export", prompt: "Add GET /invoices/:id/pdf that renders the invoice as a PDF.", state: "completed", model: OPUS, spawned: 12600, exited: 10800, turns: 2 }),
    agent("f5a6d0c8", "p3", { title: "Offline sync for drafts", prompt: "Drafts written offline should sync when the connection comes back.", state: "running", model: SONNET, spawned: 54 }),
    agent("0b8e3d77", "p3", { title: "Fix keyboard covering input", prompt: "On small phones the keyboard covers the comment input.", state: "stopped", model: SONNET, spawned: 5400, exited: 5000 }),
  ];
  const byId = (id) => agents.find((a) => a.id === id);

  // ---- events --------------------------------------------------------------
  const cbs = {};
  let cbN = 0;
  const listeners = {};
  let eventN = 0;
  function emit(name, payload) {
    for (const h of listeners[name] ?? []) cbs[h]?.({ event: name, id: ++eventN, payload: structuredClone(payload) });
  }
  const changed = (a) => emit("agent-state-changed", { ...a, host: "local" });

  let n = 0;
  const tid = () => `toolu_${(++n).toString(36).padStart(6, "0")}`;
  const assistant = (model, content) => ({ type: "assistant", message: { role: "assistant", model, content } });
  const toolResult = (id, content) => ({ type: "user", message: { role: "user", content: [{ type: "tool_result", tool_use_id: id, content, is_error: false }] } });

  // What happened before the page opened, already in the log.
  function past(startAgo) {
    let t = startAgo;
    const out = [];
    const push = (event, dt = 2) => {
      out.push({ ts: ago(t), event });
      t = Math.max(0, t - dt);
    };
    return {
      out,
      prompt: (p, turn) => push({ type: "cw_prompt", prompt: p, turn }),
      init: (model) => push({ type: "system", subtype: "init", model, session_id: "s", cwd: "/w" }, 1),
      say: (text, model) => push(assistant(model, [{ type: "text", text }]), 3),
      tool(name, input, result, model, running) {
        const id = tid();
        push(assistant(model, [{ type: "tool_use", id, name, input }]), 1);
        if (!running) push(toolResult(id, result), 2);
      },
      done: (result, ms, tokens) => push({ type: "result", subtype: "success", is_error: false, result, duration_ms: ms, usage: { output_tokens: tokens } }, 5),
    };
  }

  // What happens from now on: steps played out in time.
  function future(model) {
    const steps = [];
    const at = (wait, ev, extra) => steps.push({ wait, ev, ...extra });
    const api = {
      steps,
      prompt(p, turn) { at(150, { type: "cw_prompt", prompt: p, turn }); return api; },
      init() { at(700, { type: "system", subtype: "init", model, session_id: "s", cwd: "/w" }); return api; },
      say(text, wait = 2200) { at(wait, assistant(model, [{ type: "text", text }])); return api; },
      tool(name, input, result, wait = 1400, dur = 1100) {
        const id = tid();
        at(wait, assistant(model, [{ type: "tool_use", id, name, input }]));
        at(dur, toolResult(id, result), { change: name === "Edit" || name === "Write" ? { name, input } : null });
        return api;
      },
      done(text, tokens = 2400, wait = 1400) {
        at(wait, { type: "result", subtype: "success", is_error: false, result: text, duration_ms: 0, usage: { output_tokens: tokens } });
        return api;
      },
    };
    return api;
  }

  const events = {};
  const scripts = {}; // agent id -> steps still to play
  const changes = {}; // agent id -> [{ name, input }], what the diff shows

  {
    const L = past(262), m = OPUS;
    L.prompt("Add a dark mode toggle to the header. Remember the choice between visits.", 1);
    L.init(m);
    L.say("I'll start with how the header and the colours are wired.", m);
    L.tool("Read", { file_path: "/w/src/components/Header.tsx" }, "export function Header() { … }", m);
    L.tool("Read", { file_path: "/w/src/styles/theme.css" }, ":root { --bg: #fff; … }", m);
    L.tool("Read", { file_path: "/w/src/hooks/useLocalStorage.ts" }, "export function useLocalStorage…", m);
    L.say(
      "Colours are already CSS variables, so dark mode is one extra block, not a rewrite.\n\n**Plan**\n1. Add a `[data-theme=\"dark\"]` block to `theme.css`\n2. A `useTheme` hook that reads `localStorage`, then the OS setting\n3. A `ThemeToggle` button in the header, sun and moon icons\n4. Set the theme before first paint, so there's no white flash",
      m,
    );
    L.tool("Edit", { file_path: "/w/src/styles/theme.css", old_string: ":root {", new_string: "[data-theme=\"dark\"] {\n  --bg: #0f1115;\n}\n:root {" }, "ok", m);
    L.tool("Write", { file_path: "/w/src/hooks/useTheme.ts", content: "export function useTheme() {}" }, "ok", m);
    L.tool("Edit", { file_path: "/w/src/components/Header.tsx", old_string: "<CartIcon />", new_string: "<ThemeToggle />\n<CartIcon />" }, "ok", m);
    L.tool("Bash", { command: "npm test -- header theme", description: "Run the header and theme tests" }, " ✓ src/components/Header.test.tsx (9)\n ✓ src/hooks/useTheme.test.ts (5)\n\n Test Files  2 passed (2)\n      Tests  14 passed (14)", m);
    L.say("Done. The toggle sits in the header, the choice is kept in `localStorage`, and first visits follow the OS. All 14 tests pass.", m);
    L.done("Done.", 184000, 4210);
    L.prompt("Also make the checkout page respect it — it's still hardcoded white.", 2);
    L.init(m);
    L.tool("Grep", { pattern: "#fff|white", path: "src/pages/checkout" }, "src/pages/checkout/Summary.tsx:12\nsrc/pages/checkout/Payment.tsx:40", m);
    L.say("Two hardcoded colours in checkout. Swapping them for the theme variables.", m);
    events.a1c0d4e2 = L.out;
    changes.a1c0d4e2 = [
      { name: "Edit", input: { file_path: "/w/src/styles/theme.css", old_string: ":root {", new_string: "[data-theme=\"dark\"] {\n  --bg: #0f1115;\n  --surface: #171a21;\n}\n:root {" } },
      { name: "Write", input: { file_path: "/w/src/hooks/useTheme.ts", content: "export function useTheme() {\n  const [theme, setTheme] = useLocalStorage(\"theme\", systemTheme());\n  useEffect(() => {\n    document.documentElement.dataset.theme = theme;\n  }, [theme]);\n  return [theme, setTheme] as const;\n}" } },
      { name: "Edit", input: { file_path: "/w/src/components/Header.tsx", old_string: "<CartIcon />", new_string: "<ThemeToggle />\n<CartIcon />" } },
    ];
    scripts.a1c0d4e2 = future(m)
      .tool("Edit", { file_path: "/w/src/pages/checkout/Summary.tsx", old_string: "background: #fff;", new_string: "background: var(--bg);" }, "ok", 1800)
      .tool("Edit", { file_path: "/w/src/pages/checkout/Payment.tsx", old_string: "color: white;", new_string: "color: var(--surface);" }, "ok")
      .tool("Bash", { command: "npm test -- checkout", description: "Run the checkout tests" }, " ✓ src/pages/checkout/Summary.test.tsx (6)\n ✓ src/pages/checkout/Payment.test.tsx (8)\n\n      Tests  14 passed (14)", 1400, 2600)
      .say("Checkout follows the theme now: both hardcoded colours use the theme variables. 14 checkout tests pass.")
      .done("Done.", 1890).steps;
  }
  {
    const L = past(128), m = SONNET;
    L.prompt("Product grid images should lazy-load, with a blurred placeholder.", 1);
    L.init(m);
    L.tool("Read", { file_path: "/w/src/components/ProductCard.tsx" }, "…", m);
    L.tool("Read", { file_path: "/w/src/components/ProductGrid.tsx" }, "…", m);
    L.say("Using native `loading=\"lazy\"` plus a tiny blurred preview that fades out on load.", m);
    events.b7e19f03 = L.out;
    scripts.b7e19f03 = future(m)
      .tool("Edit", { file_path: "/w/src/components/ProductCard.tsx", old_string: "<img src={product.image}", new_string: "<img loading=\"lazy\" src={product.image}" }, "ok", 3000)
      .tool("Write", { file_path: "/w/src/components/BlurImage.tsx", content: "export function BlurImage({ src, preview }: Props) {\n  const [loaded, setLoaded] = useState(false);\n  return (\n    <div className=\"blur-image\" data-loaded={loaded}>\n      <img src={preview} aria-hidden />\n      <img src={src} loading=\"lazy\" onLoad={() => setLoaded(true)} />\n    </div>\n  );\n}" }, "ok", 2600)
      .tool("Edit", { file_path: "/w/src/components/ProductCard.tsx", old_string: "<img loading=\"lazy\" src={product.image}", new_string: "<BlurImage src={product.image} preview={product.preview}" }, "ok")
      .tool("Bash", { command: "npm run build", description: "Build the storefront" }, "✓ built in 4.1s", 1600, 3200)
      .say("Product images lazy-load now, behind a 20px blurred preview that fades out once the real image arrives.")
      .done("Done.", 2210).steps;
  }
  {
    const L = past(377), m = OPUS;
    L.prompt("Failed webhook deliveries should retry with exponential backoff, max 6 attempts.", 1);
    L.init(m);
    L.tool("Read", { file_path: "/w/src/webhooks.rs" }, "…", m);
    L.tool("Read", { file_path: "/w/src/queue.rs" }, "…", m);
    L.say("Retries go through the existing job queue, so a restart doesn't lose them.", m);
    events.d93b27aa = L.out;
    scripts.d93b27aa = future(m)
      .tool("Edit", { file_path: "/w/src/webhooks.rs", old_string: "pub async fn deliver(", new_string: "pub async fn deliver_with_retry(" }, "ok", 4200)
      .tool("Edit", { file_path: "/w/src/webhooks.rs", old_string: "const MAX_ATTEMPTS: u32 = 1;", new_string: "const MAX_ATTEMPTS: u32 = 6;\n\n/// 30s, 1m, 2m, 4m, 8m.\nfn backoff(attempt: u32) -> Duration {\n    Duration::from_secs(30 << attempt.min(4))\n}" }, "ok")
      .tool("Bash", { command: "cargo test webhooks", description: "Run the webhook tests" }, "running 11 tests\n...........\ntest result: ok. 11 passed; 0 failed", 1500, 4800)
      .say("Failed deliveries now retry through the job queue with exponential backoff: 30s, 1m, 2m, 4m, 8m, then they're marked dead. 11 tests pass.")
      .done("Done.", 3020).steps;
  }
  {
    const L = past(54), m = SONNET;
    L.prompt("Drafts written offline should sync when the connection comes back.", 1);
    L.init(m);
    events.f5a6d0c8 = L.out;
    scripts.f5a6d0c8 = future(m)
      .tool("Read", { file_path: "/w/app/drafts/store.ts" }, "export const drafts = …", 2400)
      .tool("Read", { file_path: "/w/app/net/online.ts" }, "export function useOnline() …", 1600)
      .say("Drafts already live in IndexedDB. What's missing is a queue of unsynced ones and a listener for `online`.")
      .tool("Write", { file_path: "/w/app/drafts/sync.ts", content: "export function syncWhenOnline(store: DraftStore) {\n  window.addEventListener(\"online\", () => store.flush());\n  if (navigator.onLine) store.flush();\n}" }, "ok", 3600)
      .tool("Edit", { file_path: "/w/app/drafts/store.ts", old_string: "save(draft: Draft) {", new_string: "save(draft: Draft) {\n    draft.synced = false;" }, "ok")
      .tool("Bash", { command: "npm test drafts", description: "Run the draft tests" }, "Tests  9 passed (9)", 1500, 2800)
      .say("Offline drafts are marked unsynced and flushed as soon as the connection is back. 9 tests pass.")
      .done("Done.", 1730).steps;
  }
  {
    const L = past(12600), m = OPUS;
    L.prompt("Add GET /invoices/:id/pdf that renders the invoice as a PDF.", 1);
    L.init(m);
    L.tool("Read", { file_path: "/w/src/routes/invoices.rs" }, "…", m);
    L.tool("Write", { file_path: "/w/src/pdf.rs", content: "…" }, "ok", m);
    L.tool("Bash", { command: "cargo test pdf", description: "Run the PDF tests" }, "test result: ok. 6 passed", m);
    L.say("Added `GET /invoices/:id/pdf`. It renders with the invoice template and streams the file. 6 new tests pass.", m);
    L.done("Added.", 208000, 5120);
    events.e40c5b19 = L.out;
  }
  for (const a of agents) events[a.id] ??= past(a.exited ?? 100).out;

  // Follow-ups to anything the visitor says: plausible, and about that project.
  const FILES = {
    p1: ["src/components/Header.tsx", "src/pages/checkout/Summary.tsx", "src/styles/theme.css"],
    p2: ["src/webhooks.rs", "src/routes/invoices.rs", "src/queue.rs"],
    p3: ["app/drafts/store.ts", "app/screens/Comments.tsx", "app/net/online.ts"],
  };
  const keyword = (s) => s.split(/[^A-Za-z]+/).sort((x, y) => y.length - x.length)[0]?.toLowerCase() ?? "";
  function replyTo(a, prompt) {
    const [f1, f2] = FILES[a.project_id];
    const ask = prompt.trim().replace(/\s+/g, " ");
    const short = ask.length > 60 ? ask.slice(0, 57) + "…" : ask;
    return future(a.model)
      .init()
      .say("On it. Let me find where that lives first.", 1600)
      .tool("Grep", { pattern: keyword(ask), path: "src" }, `${f1}:14\n${f2}:52`, 1200, 900)
      .tool("Read", { file_path: `/w/${f1}` }, "…", 1000, 700)
      .tool("Edit", { file_path: `/w/${f1}`, old_string: "// TODO", new_string: `// ${short}` }, "ok", 1800)
      .tool("Bash", { command: "npm test", description: "Run the tests" }, "Tests  23 passed (23)", 1300, 2400)
      .say(`Done: "${short}". The change is in \`${f1.split("/").pop()}\`, and all 23 tests pass.`)
      .done("Done.", 1100).steps;
  }

  // ---- playing turns -------------------------------------------------------
  const timers = {};
  function play(a) {
    const steps = scripts[a.id];
    if (!steps?.length) return finishTurn(a);
    const step = steps.shift();
    timers[a.id] = setTimeout(() => {
      const ev = { ts: now(), event: step.ev };
      if (step.ev.type === "result") step.ev.duration_ms = Date.now() - Date.parse(a.turn_started_at);
      events[a.id].push(ev);
      if (step.change) (changes[a.id] ??= []).push(step.change);
      emit("agent-event", { host: "local", agent_id: a.id, event: ev });
      play(a);
    }, step.wait);
  }
  function startTurn(a, prompt) {
    a.state = "running";
    a.turns += 1;
    a.turn_started_at = now();
    a.exited_at = null;
    a.exit_code = null;
    const steps = replyTo(a, prompt);
    steps.unshift({ wait: 0, ev: { type: "cw_prompt", prompt, turn: a.turns } });
    scripts[a.id] = steps;
    changed(a);
    play(a);
  }
  function finishTurn(a) {
    delete timers[a.id];
    const next = a.queue.shift();
    if (next) return startTurn(a, next.prompt);
    a.state = "completed";
    a.exited_at = now();
    a.exit_code = 0;
    changed(a);
    keepBusy(a);
  }

  // So the page never goes quiet: a little after an agent finishes, "you"
  // ask it for something more — unless the visitor has talked to it.
  const MORE = {
    p1: ["Add a test that the toggle remembers the choice after a reload.", "Animate the icon between the sun and the moon."],
    p2: ["Log each retry with its attempt number.", "Show the retry count on the webhook admin page."],
    p3: ["Show a small \"Saved offline\" badge on drafts that haven't synced."],
  };
  const visited = new Set();
  let more = 6;
  function keepBusy(a) {
    if (visited.has(a.id) || !MORE[a.project_id].length || more <= 0) return;
    more--;
    setTimeout(() => {
      if (a.state !== "running" && !visited.has(a.id)) startTurn(a, MORE[a.project_id].shift() ?? "Tidy up.");
    }, 12000 + Math.random() * 10000);
  }

  // ---- the Diff tab --------------------------------------------------------
  const invoicePatch = `diff --git a/src/routes/invoices.rs b/src/routes/invoices.rs
index 3c1e2a0..8b7d4f1 100644
--- a/src/routes/invoices.rs
+++ b/src/routes/invoices.rs
@@ -1,6 +1,7 @@
 use axum::{extract::Path, routing::get, Router};
+use crate::pdf;

 pub fn routes() -> Router<AppState> {
     Router::new()
         .route("/invoices/:id", get(show))
+        .route("/invoices/:id/pdf", get(pdf::render))
 }
diff --git a/src/pdf.rs b/src/pdf.rs
new file mode 100644
index 0000000..a41c9e2
--- /dev/null
+++ b/src/pdf.rs
@@ -0,0 +1,18 @@
+use axum::{body::Body, extract::Path, http::header, response::Response};
+
+/// The invoice as a PDF, rendered from the same template as the page.
+pub async fn render(Path(id): Path<InvoiceId>, db: Db) -> Result<Response, AppError> {
+    let invoice = db.invoice(id).await?;
+    let html = templates::invoice(&invoice)?;
+    let bytes = printer::to_pdf(&html)?;
+
+    Ok(Response::builder()
+        .header(header::CONTENT_TYPE, "application/pdf")
+        .header(
+            header::CONTENT_DISPOSITION,
+            format!("inline; filename=\\"invoice-{}.pdf\\"", invoice.number),
+        )
+        .body(Body::from(bytes))?)
+}
`;
  const commits = { e40c5b19: [{ sha: "9d0e4b7a", subject: "Add GET /invoices/:id/pdf" }] };
  const committed = { e40c5b19: true };

  // A patch built from the agent's Edits and Writes, so the Diff tab shows
  // what the transcript said it did.
  function patchOf(list) {
    const files = new Map();
    for (const { name, input } of list) {
      const path = input.file_path.replace(/^\/w\//, "");
      const f = files.get(path) ?? { path, added: name === "Write", minus: [], plus: [] };
      if (name === "Write") f.plus.push(...input.content.split("\n"));
      else {
        f.minus.push(...input.old_string.split("\n"));
        f.plus.push(...input.new_string.split("\n"));
      }
      files.set(path, f);
    }
    let patch = "";
    const out = [];
    for (const f of files.values()) {
      const minus = f.added ? [] : f.minus;
      patch += `diff --git a/${f.path} b/${f.path}\n`;
      if (f.added) patch += "new file mode 100644\n";
      patch += `--- ${f.added ? "/dev/null" : "a/" + f.path}\n+++ b/${f.path}\n`;
      patch += `@@ -${f.added ? 0 : 12},${minus.length} +${f.added ? 1 : 12},${f.plus.length} @@\n`;
      patch += minus.map((l) => "-" + l).join("\n") + (minus.length ? "\n" : "");
      patch += f.plus.map((l) => "+" + l).join("\n") + "\n";
      out.push({ path: f.path, status: f.added ? "A" : "M", insertions: f.plus.length, deletions: minus.length });
    }
    return { patch, files: out };
  }
  function diffOf(id) {
    const a = byId(id);
    const merged = a?.merged_branch ? [a.merged_branch] : [];
    if (id === "e40c5b19" && !changes[id]) {
      return {
        base: "4f2a9c1",
        files: [
          { path: "src/pdf.rs", status: "A", insertions: 18, deletions: 0 },
          { path: "src/routes/invoices.rs", status: "M", insertions: 2, deletions: 0 },
        ],
        commits: commits[id],
        patch: invoicePatch,
        truncated: false,
        uncommitted: false,
        merged_into: merged,
      };
    }
    const { patch, files } = patchOf(changes[id] ?? []);
    return {
      base: "4f2a9c1",
      files,
      commits: commits[id] ?? [],
      patch,
      truncated: false,
      uncommitted: files.length > 0 && !committed[id],
      merged_into: merged,
    };
  }

  const models = [
    { id: "claude-fable-5-1", display_name: "Claude Fable 5.1" },
    { id: OPUS, display_name: "Claude Opus 5.5" },
    { id: SONNET, display_name: "Claude Sonnet 5" },
    { id: "claude-haiku-4-5-20251001", display_name: "Claude Haiku 4.5" },
  ];

  const hex = () => Math.random().toString(16).slice(2, 10).padEnd(8, "0");
  const hostCalls = {
    list_projects: () => projects,
    list_agents: () => agents,
    startup_orphans: () => [],
    agents_holding_work: () => [],
    list_models: () => models,
    agent_events: ({ agentId }) => events[agentId] ?? [],
    agent_diff: ({ agentId }) => diffOf(agentId),
    project_branches: () => ({ current: "main", names: ["main", "develop"], pushable: ["main", "develop"] }),
    slash_commands: () => ({ commands: [], output_styles: ["default"] }),
    usage_summary: () => ({ account: { models: [], slots: [], turns: 0 }, agents: [], limits: null }),
    keep_running: () => true,
    plugins: () => ({ installed: [], available: [], marketplaces: [] }),
    project_needs_setup: () => false,

    spawn_agent({ projectId, prompt, model }) {
      const id = hex();
      const a = agent(id, projectId, { prompt, state: "running", model: model ?? OPUS, spawned: 0, turns: 0, title: null });
      a.turns = 0;
      agents.push(a);
      visited.add(id);
      events[id] = [];
      // Claude names the agent a moment in, as it does for real.
      setTimeout(() => {
        a.title = prompt.split(/[.\n]/)[0].slice(0, 48);
        changed(a);
      }, 2500);
      setTimeout(() => startTurn(a, prompt), 0);
      return { ...a, state: "running" };
    },
    send_message({ agentId, prompt }) {
      const a = byId(agentId);
      visited.add(agentId);
      if (a.state === "running") a.queue.push({ id: "q" + hex(), prompt, model: null, effort: null, permission_mode: null });
      else setTimeout(() => startTurn(a, prompt), 0);
      return a;
    },
    send_next({ agentId }) {
      const a = byId(agentId);
      const next = a.queue.shift();
      if (next) setTimeout(() => startTurn(a, next.prompt), 0);
      return a;
    },
    queue_messages({ agentId, messages }) {
      const a = byId(agentId);
      visited.add(agentId);
      a.queue.push(...messages);
      return a;
    },
    remove_queued({ agentId, messageId }) {
      const a = byId(agentId);
      a.queue = a.queue.filter((q) => q.id !== messageId);
      return a;
    },
    clear_queue({ agentId }) {
      const a = byId(agentId);
      a.queue = [];
      return a;
    },
    stop_agent({ agentId }) {
      const a = byId(agentId);
      clearTimeout(timers[agentId]);
      delete timers[agentId];
      scripts[agentId] = [];
      a.state = "stopped";
      a.exited_at = now();
      setTimeout(() => changed(a), 300);
    },
    discard_agent({ agentId }) {
      clearTimeout(timers[agentId]);
      agents.splice(agents.indexOf(byId(agentId)), 1);
    },
    rename_agent({ agentId, title }) {
      const a = byId(agentId);
      a.user_title = title;
      return a;
    },
    set_agent_color({ agentId, color }) {
      const a = byId(agentId);
      a.color = color;
      return a;
    },
    set_agent_options({ agentId, ...rest }) {
      return byId(agentId);
    },
    agent_commit({ agentId, message }) {
      const c = { sha: hex(), subject: message.split("\n")[0] };
      (commits[agentId] ??= []).push(c);
      committed[agentId] = true;
      return c;
    },
    agent_merge({ agentId, target }) {
      const a = byId(agentId);
      a.merged_branch = target;
      a.merged_at = now();
      return { outcome: "merged", target, sha: hex() };
    },
  };

  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
    plugins: {},
    convertFileSrc: (p) => p,
    transformCallback(fn) {
      const id = ++cbN;
      cbs[id] = fn;
      return id;
    },
    unregisterCallback(id) {
      delete cbs[id];
    },
    async invoke(cmd, args = {}) {
      calls.push([cmd, args]);
      if (cmd === "hosts")
        return [{ id: "local", local: true, status: { state: "connected", instance: "i1", version: "0.1.0", name: "workstation" } }];
      if (cmd === "host") {
        const f = hostCalls[args.method];
        if (!f) console.debug("[demo] unanswered host call", args.method, args.args);
        return f ? structuredClone(f(args.args ?? {})) : null;
      }
      if (cmd === "plugin:event|listen") {
        (listeners[args.event] ??= []).push(args.handler);
        return args.handler;
      }
      if (cmd === "plugin:notification|is_permission_granted") return true;
      if (cmd === "plugin:window|is_maximized") return false;
      if (cmd === "tailnet_machines") return [];
      return null;
    },
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };

  // The app's notifications go nowhere: the page beside it stays still.
  window.Notification = class {
    static permission = "granted";
    static requestPermission = async () => "granted";
    close() {}
  };

  // Running agents pick up where their logs left off once the page is open.
  addEventListener("load", () => {
    setTimeout(() => {
      for (const a of agents) if (a.state === "running" && scripts[a.id]) play(a);
    }, 1200);
  });
})();
