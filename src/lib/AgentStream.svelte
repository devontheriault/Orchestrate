<script lang="ts">
  import { store } from "./store.svelte";
  import { formatDuration, formatTokens } from "./format";
  import AgentDiff from "./AgentDiff.svelte";
  import AgentComposer from "./AgentComposer.svelte";
  import Markdown from "./Markdown.svelte";
  import ShellCommand from "./ShellCommand.svelte";
  import type { AgentEvent } from "./api";

  /** The blank page for an agent that hasn't been spawned yet. */
  const drafting = $derived(store.drafting && !store.selectedAgent);

  let streamEl: HTMLDivElement | undefined = $state();

  /**
   * Reap deletes the worktree, the branch, and with it the agent's session —
   * everything not merged out is gone. It used to happen only as part of an
   * explicit Stop; now that Stop preserves work, Reap is the one destructive
   * button in the app, so it takes two clicks.
   */
  let reapArmed = $state(false);
  let disarm: ReturnType<typeof setTimeout> | undefined;

  function armReap() {
    reapArmed = true;
    clearTimeout(disarm);
    disarm = setTimeout(() => (reapArmed = false), 4000);
  }

  function reap() {
    clearTimeout(disarm);
    reapArmed = false;
    store.reapAgent(store.selectedAgent!.id);
  }

  // Never leave the trigger armed across a change of agent.
  $effect(() => {
    store.selectedAgentId;
    clearTimeout(disarm);
    reapArmed = false;
  });

  /**
   * How far from the bottom still counts as being at the bottom: scrolling
   * back down to within this slack picks the stream up again.
   */
  const FOLLOW_SLACK = 120;

  /**
   * Whether the transcript is following the newest output. Only the user
   * scrolling up lets go of it — new output never does. Measuring the distance
   * from the bottom once output arrives can't tell the two apart: by then the
   * DOM already holds it, so one large chunk reads as the user having scrolled
   * away. The jump-to-bottom button shows exactly while this is off.
   */
  let following = $state(true);
  let lastScrollTop = 0;

  function onScroll() {
    const el = streamEl;
    if (!el) return;
    if (el.scrollHeight - el.scrollTop - el.clientHeight < FOLLOW_SLACK) following = true;
    // Our own scrolls only ever go down, and content growing below doesn't
    // move scrollTop at all, so a scroll upward is the user's doing.
    else if (el.scrollTop < lastScrollTop) following = false;
    lastScrollTop = el.scrollTop;
  }

  function stickToBottom() {
    requestAnimationFrame(() => {
      if (following && streamEl) streamEl.scrollTop = streamEl.scrollHeight;
    });
  }

  function jumpToBottom() {
    const el = streamEl;
    if (!el) return;
    following = true;
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    el.scrollTo({ top: el.scrollHeight, behavior: reduced ? "auto" : "smooth" });
  }

  // New output scrolls itself into view unless the user has scrolled up.
  $effect(() => {
    const len = store.eventsForSelected.length;
    if (!streamEl || len === 0) return;
    if (following) stickToBottom();
  });

  // The pane shrinking (a taller composer, a resized window) would otherwise
  // push the newest output out of sight without anything being scrolled.
  function followResizes(el: HTMLDivElement) {
    const ro = new ResizeObserver(() => {
      if (following) el.scrollTop = el.scrollHeight;
    });
    ro.observe(el);
    return () => ro.disconnect();
  }

  // On selection change, always jump to the bottom of the new stream.
  $effect(() => {
    const id = store.selectedAgentId;
    if (!streamEl || !id) return;
    following = true;
    stickToBottom();
  });

  // Helpers to decode common stream-json event shapes without breaking on
  // unknowns. Anything unrecognised falls through to the raw-JSON card.
  type Kind =
    | { kind: "prompt"; text: string }
    | { kind: "notice"; text: string }
    | { kind: "system"; subtype: string }
    | { kind: "text"; text: string }
    | { kind: "tool_use"; name: string; input: unknown; id: string }
    | { kind: "tool_result"; tool_use_id: string; content: unknown }
    | { kind: "thinking"; text: string }
    | {
        kind: "result";
        result: string;
        is_error: boolean;
        duration_ms?: number;
        output_tokens?: number;
      }
    | { kind: "raw"; type?: string };

  /**
   * Pure telemetry that `claude` emits every turn (sometimes several times)
   * but carries nothing a user reading the transcript can act on. Dropped
   * rather than rendered, so the stream doesn't read as the same line
   * repeated.
   */
  const SILENT_EVENT_TYPES = new Set(["rate_limit_event"]);
  const SILENT_SYSTEM_SUBTYPES = new Set(["thinking_tokens"]);

  /**
   * Claude Code's system subtypes, in the words the user reads them in. The
   * set is open-ended — the CLI adds to it without telling us — so anything
   * unknown falls back to its own name as a sentence rather than leaking the
   * protocol's snake_case into the transcript.
   */
  const SYSTEM_LABELS: Record<string, string> = {
    "": "Session update",
    init: "Session started",
    compact_boundary: "Conversation compacted to free up context",
  };

  /** `compact_boundary` -> `Compact boundary`. */
  function humanize(s: string): string {
    const words = s.replace(/[_-]+/g, " ").trim();
    return words ? words[0].toUpperCase() + words.slice(1) : "";
  }

  function systemLabel(subtype: string): string {
    return SYSTEM_LABELS[subtype] ?? humanize(subtype);
  }

  function classify(ev: AgentEvent): Kind[] {
    const e = ev.event as {
      type?: string;
      message?: any;
      subtype?: string;
      result?: string;
      is_error?: boolean;
      prompt?: string;
      text?: string;
      duration_ms?: number;
      usage?: { output_tokens?: number };
    } | null;
    if (!e || typeof e !== "object") return [{ kind: "raw" }];
    if (SILENT_EVENT_TYPES.has(e.type ?? "")) return [];
    // Our own event, not claude's: the prompt the user sent for this turn.
    if (e.type === "cw_prompt") return [{ kind: "prompt", text: e.prompt ?? "" }];
    // Also ours: something the app did on the agent's behalf, like finishing
    // the merge a resolver was spawned for.
    if (e.type === "cw_notice") return [{ kind: "notice", text: e.text ?? "" }];
    if (e.type === "system") {
      if (SILENT_SYSTEM_SUBTYPES.has(e.subtype ?? "")) return [];
      return [{ kind: "system", subtype: e.subtype ?? "" }];
    }
    if (e.type === "result")
      return [
        {
          kind: "result",
          result: e.result ?? "",
          is_error: !!e.is_error,
          duration_ms: e.duration_ms,
          output_tokens: e.usage?.output_tokens,
        },
      ];
    if ((e.type === "assistant" || e.type === "user") && e.message?.content) {
      const blocks = Array.isArray(e.message.content) ? e.message.content : [];
      return blocks
        .map((b: any): Kind => {
          if (b.type === "text") return { kind: "text", text: b.text ?? "" };
          if (b.type === "tool_use")
            return { kind: "tool_use", name: b.name ?? "?", input: b.input, id: b.id ?? "" };
          if (b.type === "tool_result")
            return { kind: "tool_result", tool_use_id: b.tool_use_id ?? "", content: b.content };
          if (b.type === "thinking") return { kind: "thinking", text: b.thinking ?? "" };
          return { kind: "raw", type: b.type };
        })
        // Redacted/interleaved thinking often carries a signature but no
        // visible text — an empty "thinking" card tells the user nothing.
        .filter((k: Kind) => !(k.kind === "thinking" && !k.text.trim()));
    }
    return [{ kind: "raw", type: e.type }];
  }

  /**
   * One tool call and the result that came back for it. Claude Code reports
   * them as two separate events a few messages apart; the transcript reads
   * better as one thing, so we pair them on `tool_use_id` and render the
   * result inside the call that asked for it.
   */
  type ToolCall = {
    key: string;
    name: string;
    input: unknown;
    result: unknown;
    hasResult: boolean;
  };

  /**
   * A row of the transcript. Several stream events can collapse into one row:
   * a run of same-tool calls, or a stretch of thinking. Rows carry a stable
   * `key` taken from the first event they cover, so a row that grows as the
   * turn runs keeps the same DOM node — and with it whatever the user had
   * expanded.
   */
  type Row =
    | { key: string; kind: "prompt"; text: string }
    | { key: string; kind: "notice"; text: string }
    | { key: string; kind: "text"; text: string }
    | { key: string; kind: "thinking"; parts: string[] }
    | { key: string; kind: "tools"; name: string; calls: ToolCall[] }
    | { key: string; kind: "system"; subtype: string }
    | {
        key: string;
        kind: "result";
        result: string;
        is_error: boolean;
        duration_ms?: number;
        output_tokens?: number;
      }
    | { key: string; kind: "raw"; type?: string; event: unknown };

  /**
   * Tools whose input is a snapshot of a whole state rather than an action:
   * the tenth todo list supersedes the nine before it, so only the last one
   * in a turn is worth a row. Scoped to the turn rather than the whole
   * transcript — reading back an old turn should show the list as it stood
   * when that turn ended, not today's.
   */
  const SNAPSHOT_TOOLS = new Set(["TodoWrite"]);

  const rows: Row[] = $derived.by(() => {
    // Flatten every event into its blocks first, tagged with the turn they
    // fall in, so superseded snapshots can be spotted before anything is
    // grouped.
    const items: { key: string; k: Kind; turn: number; event: unknown }[] = [];
    let turn = 0;
    const evs = store.eventsForSelected;
    for (let i = 0; i < evs.length; i++) {
      const ks = classify(evs[i]);
      for (let j = 0; j < ks.length; j++) {
        if (ks[j].kind === "prompt") turn++;
        items.push({ key: `${i}:${j}`, k: ks[j], turn, event: evs[i].event });
      }
    }

    const liveSnapshot = new Map<string, string>();
    for (const it of items) {
      if (it.k.kind === "tool_use" && SNAPSHOT_TOOLS.has(it.k.name)) {
        liveSnapshot.set(`${it.turn}:${it.k.name}`, it.key);
      }
    }

    const out: Row[] = [];
    const callsById = new Map<string, ToolCall>();
    const seenSystem = new Set<string>();

    for (const it of items) {
      const k = it.k;
      const last = out[out.length - 1];

      if (k.kind === "tool_use") {
        if (SNAPSHOT_TOOLS.has(k.name) && liveSnapshot.get(`${it.turn}:${k.name}`) !== it.key) {
          continue;
        }
        const call: ToolCall = {
          key: it.key,
          name: k.name,
          input: k.input,
          result: undefined,
          hasResult: false,
        };
        if (k.id) callsById.set(k.id, call);
        if (last?.kind === "tools" && last.name === k.name) last.calls.push(call);
        else out.push({ key: it.key, kind: "tools", name: k.name, calls: [call] });
      } else if (k.kind === "tool_result") {
        // No call on record means it belonged to a superseded snapshot; the
        // result goes with it.
        const call = callsById.get(k.tool_use_id);
        if (call) {
          call.result = k.content;
          call.hasResult = true;
        }
      } else if (k.kind === "thinking") {
        if (last?.kind === "thinking") last.parts.push(k.text);
        else out.push({ key: it.key, kind: "thinking", parts: [k.text] });
      } else if (k.kind === "system") {
        // `init` and friends repeat every turn and say the same thing each
        // time; the first one is the only one that informs.
        if (seenSystem.has(k.subtype)) continue;
        seenSystem.add(k.subtype);
        out.push({ key: it.key, kind: "system", subtype: k.subtype });
      } else if (k.kind === "raw") {
        out.push({ key: it.key, kind: "raw", type: k.type, event: it.event });
      } else {
        out.push({ key: it.key, ...k });
      }
    }
    return out;
  });

  function basename(path: string): string {
    return path.split("/").filter(Boolean).pop() ?? path;
  }

  function truncate(s: string, n: number): string {
    const flat = s.replace(/\s+/g, " ").trim();
    return flat.length > n ? flat.slice(0, n - 1) + "…" : flat;
  }

  /**
   * What a call was *about*, in a few words: the file read, the pattern
   * searched for, the command run. It's the difference between a row that
   * says `Read ×8` and one that says which eight.
   */
  function callTarget(name: string, input: unknown): string {
    const o = (input && typeof input === "object" ? input : {}) as Record<string, any>;
    const str = (v: unknown) => (typeof v === "string" && v.trim() ? v.trim() : "");
    switch (name) {
      case "Read":
      case "Write":
      case "Edit":
        return basename(str(o.file_path));
      case "NotebookEdit":
        return basename(str(o.notebook_path));
      case "Bash":
        return str(o.description) || str(o.command);
      case "Grep":
      case "Glob":
        return str(o.pattern);
      case "Task":
        return str(o.description);
      case "Skill":
        return str(o.skill);
      case "WebSearch":
        return str(o.query);
      case "WebFetch":
        try {
          return new URL(str(o.url)).host;
        } catch {
          return str(o.url);
        }
      case "TodoWrite":
        return Array.isArray(o.todos) ? `${o.todos.length} items` : "";
    }
    // An unknown tool's first string argument is usually its subject.
    for (const v of Object.values(o)) {
      const s = str(v);
      if (s) return s;
    }
    return "";
  }

  /**
   * The targets of a grouped run. While a call in it is still going, it's
   * the only one named — each new call overwrites the last, so the row reads
   * as what's happening now. Once the run settles: the first couple, then a
   * count.
   */
  function groupTargets(calls: ToolCall[]): string {
    const running = calls.findLast((c) => !c.hasResult);
    if (running && calls.length > 1) return truncate(callTarget(running.name, running.input), 80);
    const seen: string[] = [];
    for (const c of calls) {
      const t = truncate(callTarget(c.name, c.input), 40);
      if (t && !seen.includes(t)) seen.push(t);
    }
    if (seen.length === 0) return "";
    const shown = seen.slice(0, 2);
    const rest = seen.length - shown.length;
    return rest > 0 ? `${shown.join(", ")}, +${rest}` : shown.join(", ");
  }

  /**
   * The newest Bash call in the transcript. It opens itself, so the command
   * being run — and then what it printed — is in view without a click, and
   * stays open until the next Bash call takes its place.
   */
  const lastBashKey = $derived.by(() => {
    const row = rows.findLast((r) => r.kind === "tools" && r.name === "Bash");
    return row?.kind === "tools" ? row.calls.at(-1)?.key : undefined;
  });

  /**
   * Keeps a `<details>` following `want`, but only touches it when `want`
   * flips. Rows are rebuilt on every event, and a plain `open={…}` would be
   * re-applied each time — folding up whatever the user had opened by hand.
   */
  const applied = new WeakMap<HTMLDetailsElement, boolean>();
  function autoOpen(want: boolean) {
    return (el: HTMLDetailsElement) => {
      if (applied.get(el) === want) return;
      applied.set(el, want);
      el.open = want;
    };
  }

  /** A Bash call's command, if that's what `c` is. */
  function bashCommand(c: ToolCall): string | undefined {
    const o = c.input as Record<string, unknown> | null;
    return c.name === "Bash" && typeof o?.command === "string" ? o.command : undefined;
  }

  function toolResultText(content: unknown): string {
    if (typeof content === "string") return content;
    if (Array.isArray(content)) {
      return content
        .map((c: any) => (c?.type === "text" ? c.text : JSON.stringify(c)))
        .join("\n");
    }
    return JSON.stringify(content);
  }
</script>

<!-- A call's input as shown when expanded. A command reads better laid out
     and coloured than JSON-escaped. -->
{#snippet callInput(c: ToolCall)}
  {@const command = bashCommand(c)}
  {#if command !== undefined}
    <ShellCommand {command} />
  {:else}
    <pre>{JSON.stringify(c.input, null, 2)}</pre>
  {/if}
{/snippet}

<section>
  <!-- The heading moved to the window bar above, which is painted like this
       pane; what's left here are the pane's own controls. -->
  {#if store.selectedAgent || drafting}
    <header>
      {#if store.selectedAgent}
        <div class="tabs" role="tablist">
          <button
            role="tab"
            aria-selected={store.detailTab === "output"}
            class:active={store.detailTab === "output"}
            onclick={() => store.showTab("output")}>Output</button
          >
          <button
            role="tab"
            aria-selected={store.detailTab === "diff"}
            class:active={store.detailTab === "diff"}
            onclick={() => store.showTab("diff")}
          >
            Diff
            {#if store.diff?.uncommitted}
              <span class="dirty-dot" title="uncommitted work"></span>
            {/if}
          </button>
        </div>
        <!-- Stop lives in the composer's send slot while the agent runs; the
             header keeps only the destructive action, once it's stopped. -->
        {#if store.selectedAgent.state !== "running"}
          <div class="head-right">
            <button
              class="reap"
              class:armed={reapArmed}
              onclick={() => (reapArmed ? reap() : armReap())}
              onblur={() => (reapArmed = false)}
              title="Delete this agent's worktree and branch — uncommitted work and its conversation go with them"
            >
              {reapArmed ? "Reap for good?" : "Reap"}
            </button>
          </div>
        {/if}
      {:else}
        <div class="head-right">
          <button class="cancel" onclick={() => store.cancelDraft()}>Cancel</button>
        </div>
      {/if}
    </header>
  {/if}

  {#if store.selectedAgent && store.detailTab === "diff"}
    <AgentDiff />
  {:else}
    <div class="stream-wrap">
      <div class="stream" bind:this={streamEl} onscroll={onScroll} {@attach followResizes}>
        {#if store.selectedAgent?.state === "failed" && store.selectedAgent.fail_reason}
          <div class="fail-banner">
            <span class="label">Failed</span>
            <span class="reason">{store.selectedAgent.fail_reason}</span>
          </div>
        {/if}

        {#if drafting}
          <div class="hint draft-hint">
            <p>This agent's output will appear here.</p>
            <p class="sub">
              Describe the task below and spawn it — it gets a fresh worktree and
              branch of its own.
            </p>
          </div>
        {:else if !store.selectedAgent}
          <div class="hint">Open a project and pick one of its agents to see its output.</div>
        {:else if store.eventsForSelected.length === 0}
          <div class="hint">
            {store.selectedAgent.state === "running"
              ? "Waiting for output…"
              : "No events on record."}
          </div>
        {:else}
          {#each rows as row (row.key)}
            {#if row.kind === "prompt"}
              <div class="block prompt-block">{row.text}</div>
            {:else if row.kind === "text"}
              <div class="block text"><Markdown text={row.text} /></div>
            {:else if row.kind === "tools"}
              <details
                class="block tool"
                {@attach autoOpen(row.calls.some((c) => c.key === lastBashKey))}
              >
                <summary>
                  <span class="tool-name">→ {row.name}</span>
                  {#if row.calls.length > 1}<span class="count">×{row.calls.length}</span>{/if}
                  {#if groupTargets(row.calls)}
                    <span class="targets">{groupTargets(row.calls)}</span>
                  {/if}
                  {#if row.calls.some((c) => !c.hasResult)}
                    <span class="running-dot" title="still running"></span>
                  {/if}
                </summary>
                {#if row.calls.length === 1}
                  {@const c = row.calls[0]}
                  {@render callInput(c)}
                  {#if c.hasResult}
                    <div class="call-result"><pre>{toolResultText(c.result)}</pre></div>
                  {/if}
                {:else}
                  <!-- A run of same-tool calls: the group is one row, each
                       call inside it still opens on its own. -->
                  {#each row.calls as c (c.key)}
                    <details class="call" {@attach autoOpen(c.key === lastBashKey)}>
                      <summary>
                        {truncate(callTarget(c.name, c.input), 80) || c.name}
                        {#if !c.hasResult}<span class="running-dot"></span>{/if}
                      </summary>
                      {@render callInput(c)}
                      {#if c.hasResult}
                        <div class="call-result"><pre>{toolResultText(c.result)}</pre></div>
                      {/if}
                    </details>
                  {/each}
                {/if}
              </details>
            {:else if row.kind === "thinking"}
              <details class="block thinking">
                <summary
                  >thinking{#if row.parts.length > 1}<span class="count"
                      >×{row.parts.length}</span
                    >{/if}</summary
                >
                <div class="thinking-body"><Markdown text={row.parts.join("\n\n")} /></div>
              </details>
            {:else if row.kind === "system"}
              <div class="block system">{systemLabel(row.subtype)}</div>
            {:else if row.kind === "notice"}
              <div class="block system notice"><Markdown text={row.text} /></div>
            {:else if row.kind === "result"}
              <div class="block done" class:err={row.is_error}>
                {row.is_error ? "✗ error" : "✓ done"}
                {#if row.duration_ms != null}
                  — {formatDuration(row.duration_ms)}
                {/if}
                {#if row.output_tokens != null}
                  · {formatTokens(row.output_tokens)} tok
                {/if}
                {#if row.is_error && row.result}
                  — {row.result}
                {/if}
              </div>
            {:else}
              <details class="block raw-detail">
                <summary>{humanize(row.type ?? "") || "Unrecognized event"}</summary>
                <pre>{JSON.stringify(row.event, null, 2)}</pre>
              </details>
            {/if}
          {/each}
        {/if}
      </div>
      {#if !following}
        <!-- Only while the transcript has been scrolled away from: the way
             back down is one click, in the gap above the composer. -->
        <button
          class="to-bottom"
          onclick={jumpToBottom}
          aria-label="Jump to the latest output"
          title="Jump to the latest output"
        >
          <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
            <path
              d="M8 3v9M8 12l-4-4M8 12l4-4"
              fill="none"
              stroke="currentColor"
              stroke-width="1.8"
              stroke-linecap="round"
              stroke-linejoin="round"
            />
          </svg>
        </button>
      {/if}
    </div>
    <AgentComposer />
  {/if}
</section>

<style>
  section {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    background: var(--surface);
    overflow: hidden;
  }

  header {
    padding: var(--pad-y) var(--pad-x);
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem 1rem;
    min-height: 2.7rem;
  }

  .cancel:hover { border-color: var(--accent); color: var(--accent); }

  .head-right {
    display: flex;
    flex: none;
    align-items: center;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 0.4rem 0.5rem;
    margin-left: auto;
  }

  .head-right button {
    flex: none;
    white-space: nowrap;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    background: var(--surface);
    padding: 0.35rem 0.85rem;
    font-size: var(--text-md);
    font-family: inherit;
    color: var(--fg);
    cursor: pointer;
  }

  .tabs {
    display: inline-flex;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    overflow: hidden;
  }

  .tabs button {
    border: none;
    border-radius: 0;
    background: transparent;
    padding: 0.32rem 0.8rem;
    font-size: var(--text-md);
    color: var(--fg-muted);
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: var(--space-3);
  }

  .tabs button:hover { color: var(--fg); }

  .tabs button.active {
    background: var(--selected);
    color: var(--fg);
    font-weight: var(--weight-medium);
  }

  .dirty-dot {
    width: 6px;
    height: 6px;
    border-radius: var(--radius-circle);
    background: var(--warning);
  }

  .reap:hover { border-color: var(--accent); color: var(--accent); }

  .reap.armed,
  .reap.armed:hover {
    border-color: var(--danger);
    background: var(--danger);
    color: var(--on-danger);
    font-weight: var(--weight-medium);
  }

  /* Holds the transcript and the jump-to-bottom button that floats over it. */
  .stream-wrap {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .stream {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
    padding: 1rem var(--pad-x) 2rem;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  /* Centred on the transcript's own edge, just above the composer: the button
     sits where the newest output is, which is where the eye already is. */
  .to-bottom {
    position: absolute;
    bottom: 0.6rem;
    left: 50%;
    transform: translateX(-50%);
    width: 2rem;
    height: 2rem;
    display: grid;
    place-items: center;
    border: 1px solid var(--border);
    border-radius: var(--radius-circle);
    background: var(--panel-bg);
    color: var(--fg-muted);
    box-shadow: var(--shadow-sm);
    cursor: pointer;
    padding: 0;
    animation: to-bottom-in 0.12s ease;
  }

  .to-bottom:hover {
    border-color: var(--accent);
    color: var(--accent);
  }

  @keyframes to-bottom-in {
    from { opacity: 0; transform: translate(-50%, 0.3rem); }
    to { opacity: 1; transform: translate(-50%, 0); }
  }

  .hint {
    color: var(--fg-muted);
    font-style: italic;
    text-align: center;
    padding: 3rem 1rem;
    font-size: var(--text-lg);
  }

  /* The blank page is mostly empty on purpose: it's the transcript, waiting.
     Centre the little it does say in the space the output will fill. */
  .draft-hint {
    margin: auto 0;
    font-style: normal;
  }

  .draft-hint p {
    margin: 0;
  }

  .draft-hint .sub {
    margin-top: var(--space-3);
    font-size: var(--text-md);
    max-width: var(--measure);
    margin-left: auto;
    margin-right: auto;
  }

  .block {
    font-size: var(--text-lg);
    line-height: var(--leading-relaxed);
    min-width: 0;
  }

  /* Claude answers in markdown, so the transcript renders it as a document
     rather than as the source. `Markdown` brings its own spacing. */
  .text {
    padding: 0.15rem 0;
    /* Prose stops at a readable measure however wide the window gets. */
    max-width: min(100%, var(--measure));
  }

  /* The user's side of the conversation: indented, so turns are easy to find
     when scanning a long transcript. */
  .prompt-block {
    align-self: flex-start;
    max-width: min(100%, var(--measure));
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    background: var(--selected);
    border-left: 2px solid var(--accent);
    border-radius: 0 var(--radius-md) var(--radius-md) 0;
    padding: 0.45rem 0.7rem;
    margin: 0.45rem 0 0.2rem;
    font-size: var(--text-lg);
  }

  details.block {
    background: var(--panel-bg);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    padding: 0.4rem 0.75rem;
  }

  details.block summary {
    cursor: pointer;
    font-family: var(--font-mono);
    font-size: var(--text-md);
    color: var(--fg-muted);
    overflow-wrap: anywhere;
  }

  details.block[open] summary { margin-bottom: var(--space-3); }

  details.block pre {
    background: transparent;
    margin: 0;
    padding: 0;
    font-size: var(--text-sm);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    color: var(--fg);
    /* A very long line scrolls inside the card instead of stretching it. */
    max-height: min(60vh, 34rem);
    overflow: auto;
  }

  /* Thinking is prose too, but a long stretch of it scrolls inside its card
     instead of burying the answer that follows. */
  .thinking-body {
    font-size: var(--text-md);
    color: var(--fg-muted);
    max-height: min(60vh, 34rem);
    overflow: auto;
  }

  /* A grouped run reads left to right: what the tool was, how many times,
     then what it was pointed at. */
  details.block summary .tool-name {
    color: var(--fg);
  }

  details.block summary .count {
    margin-left: var(--space-2);
    color: var(--accent);
    font-size: var(--text-sm);
  }

  details.block summary .targets {
    margin-left: var(--space-3);
    color: var(--fg-muted);
  }

  details.block summary .targets::before {
    content: "— ";
  }

  /* A call whose result hasn't come back yet — the row is still being
     written to. */
  .running-dot {
    display: inline-block;
    vertical-align: middle;
    margin-left: var(--space-3);
    width: 6px;
    height: 6px;
    border-radius: var(--radius-circle);
    background: var(--accent);
    animation: running-pulse 1.2s ease-in-out infinite;
  }

  @keyframes running-pulse {
    0%, 100% { opacity: 0.25; }
    50% { opacity: 1; }
  }

  /* Kept, unlike the app's other pulses: this one's last keyframe is its
     dim one, so stopping it needs a resting opacity of its own. */
  @media (prefers-reduced-motion: reduce) {
    .running-dot { animation: none; opacity: 0.7; }
  }

  details.call {
    border-top: 1px solid var(--border);
    padding: 0.35rem 0 0.3rem 0.5rem;
  }

  details.call summary {
    cursor: pointer;
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    color: var(--fg-muted);
    overflow-wrap: anywhere;
  }

  details.call[open] summary { margin-bottom: var(--space-2); }

  details.call pre {
    background: transparent;
    margin: 0;
    padding: 0;
    font-size: var(--text-sm);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    color: var(--fg);
    max-height: min(50vh, 28rem);
    overflow: auto;
  }

  /* The result sits under the call that asked for it, divided from the input
     rather than split into a card of its own. */
  .call-result {
    margin-top: var(--space-3);
    padding-top: 0.4rem;
    border-top: 1px dashed var(--border);
  }

  .call-result pre {
    color: var(--fg-muted);
  }

  .system, .done {
    color: var(--fg-muted);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    padding: 0.15rem 0;
    overflow-wrap: anywhere;
    max-width: min(100%, var(--measure));
  }

  .done.err { color: var(--danger-text); }

  /* The app's own word in the transcript: a single line, like the rows around it. */
  .notice :global(p) { margin: 0; }

  .fail-banner {
    background: var(--danger-soft-bg);
    border: 1px solid var(--danger-soft-border);
    border-radius: var(--radius-md);
    padding: 0.55rem 0.85rem;
    font-size: var(--text-md);
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem 0.6rem;
    align-items: baseline;
  }

  .fail-banner .label {
    color: var(--danger-text);
    font-weight: var(--weight-semibold);
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .fail-banner .reason {
    color: var(--fg);
    font-family: var(--font-mono);
    font-size: var(--text-md);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    min-width: 0;
  }
</style>
