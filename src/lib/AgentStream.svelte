<script lang="ts">
  import { store } from "./store.svelte";
  import AgentDiff from "./AgentDiff.svelte";
  import AgentComposer from "./AgentComposer.svelte";
  import Markdown from "./Markdown.svelte";
  import type { AgentEvent } from "./api";

  /** Present only when the project tree is off-screen, on a narrow window. */
  let { onBack }: { onBack?: () => void } = $props();

  /** The blank page for an agent that hasn't been spawned yet. */
  const drafting = $derived(store.drafting && !store.selectedAgent);

  const projectName = $derived(
    store.projects.find((p) => p.id === store.selectedProjectId)?.name ?? "",
  );

  let showRaw = $state(false);
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

  // On new events, only auto-scroll if the user is already near the bottom.
  $effect(() => {
    const len = store.eventsForSelected.length;
    if (!streamEl || len === 0) return;
    const el = streamEl;
    const nearBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 120;
    if (nearBottom) {
      requestAnimationFrame(() => {
        el.scrollTop = el.scrollHeight;
      });
    }
  });

  // On selection change, always jump to the bottom of the new stream.
  $effect(() => {
    const id = store.selectedAgentId;
    if (!streamEl || !id) return;
    requestAnimationFrame(() => {
      streamEl!.scrollTop = streamEl!.scrollHeight;
    });
  });

  // Helpers to decode common stream-json event shapes without breaking on
  // unknowns. Anything unrecognised falls through to the raw-JSON card.
  type Kind =
    | { kind: "prompt"; text: string }
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

  function classify(ev: AgentEvent): Kind[] {
    const e = ev.event as {
      type?: string;
      message?: any;
      subtype?: string;
      result?: string;
      is_error?: boolean;
      prompt?: string;
      duration_ms?: number;
      usage?: { output_tokens?: number };
    } | null;
    if (!e || typeof e !== "object") return [{ kind: "raw" }];
    if (SILENT_EVENT_TYPES.has(e.type ?? "")) return [];
    // Our own event, not claude's: the prompt the user sent for this turn.
    if (e.type === "cw_prompt") return [{ kind: "prompt", text: e.prompt ?? "" }];
    if (e.type === "system") {
      if (SILENT_SYSTEM_SUBTYPES.has(e.subtype ?? "")) return [];
      return [{ kind: "system", subtype: e.subtype ?? "?" }];
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

  /** "1h 2m", "3m 4s", or "12s" — for a duration that's already over. */
  function formatDuration(ms: number): string {
    const total = Math.max(0, Math.round(ms / 1000));
    const h = Math.floor(total / 3600);
    const m = Math.floor((total % 3600) / 60);
    const s = total % 60;
    if (h > 0) return `${h}h ${m}m`;
    if (m > 0) return `${m}m ${s}s`;
    return `${s}s`;
  }

  function formatTokens(n: number): string {
    return n >= 1000 ? `${(n / 1000).toFixed(1).replace(/\.0$/, "")}k` : `${n}`;
  }

  // Ticks once a second while the selected agent is running, so the elapsed
  // timer in the header advances without waiting for the next stream event.
  let now = $state(Date.now());
  $effect(() => {
    if (store.selectedAgent?.state !== "running") return;
    const id = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(id);
  });

  /**
   * When the running turn began: the timestamp of the latest prompt the user
   * sent, falling back to when the agent was first spawned. Either way it's
   * an event already on the log, so no extra field is needed on the Agent.
   */
  const turnStartedAt = $derived.by(() => {
    const events = store.eventsForSelected;
    for (let i = events.length - 1; i >= 0; i--) {
      if ((events[i].event as any)?.type === "cw_prompt") return Date.parse(events[i].ts);
    }
    return store.selectedAgent ? Date.parse(store.selectedAgent.spawned_at) : null;
  });

  const liveElapsedMs = $derived(
    turnStartedAt != null ? Math.max(0, now - turnStartedAt) : 0,
  );

  /**
   * Tokens generated so far in the running turn: `claude` reports usage per
   * API call rather than as incremental deltas, so this sums each call's
   * output as it arrives — an estimate that climbs as the turn progresses,
   * not the exact total the final `result` event reports.
   */
  const liveOutputTokens = $derived.by(() => {
    const events = store.eventsForSelected;
    let sinceIdx = -1;
    for (let i = events.length - 1; i >= 0; i--) {
      if ((events[i].event as any)?.type === "cw_prompt") {
        sinceIdx = i;
        break;
      }
    }
    let total = 0;
    for (let i = sinceIdx + 1; i < events.length; i++) {
      const e = events[i].event as any;
      const ot = e?.type === "assistant" ? e.message?.usage?.output_tokens : undefined;
      if (typeof ot === "number") total += ot;
    }
    return total;
  });

  function shortenInput(input: unknown): string {
    const s = typeof input === "string" ? input : JSON.stringify(input);
    return s.length > 120 ? s.slice(0, 117) + "…" : s;
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

<section>
  <header>
    {#if onBack}
      <button class="back" onclick={onBack} title="Back to projects" aria-label="Back to projects"
        >←</button
      >
    {/if}
    {#if store.selectedAgent}
      <div class="head-left">
        <div class="prompt">{store.selectedAgent.task.prompt}</div>
        <div class="meta">
          <span class={`state state-${store.selectedAgent.state}`}>
            {store.selectedAgent.state}
          </span>
          {#if store.selectedAgent.state === "running"}
            <span class="live-stat" title="Time since this turn started">
              {formatDuration(liveElapsedMs)}
            </span>
            <span class="live-stat" title="Tokens generated so far this turn (estimate)">
              ~{formatTokens(liveOutputTokens)} tok
            </span>
          {/if}
          <code>{store.selectedAgent.id}</code>
          {#if store.selectedAgent.branch}
            <code>{store.selectedAgent.branch}</code>
          {/if}
          {#if store.selectedAgent.model}
            <span class="model" title="Model this agent runs on">
              {store.modelName(store.selectedAgent.model)}
            </span>
          {/if}
        </div>
      </div>
      <div class="head-right">
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
        {#if store.detailTab === "output"}
          <label class="raw-toggle" title="Show the unparsed event JSON">
            <input type="checkbox" bind:checked={showRaw} />
            <span>raw</span>
          </label>
        {/if}
        {#if store.selectedAgent.state === "running"}
          <button
            class="stop"
            onclick={() => store.stopAgent(store.selectedAgent!.id)}>Stop</button
          >
        {:else}
          <button
            class="reap"
            class:armed={reapArmed}
            onclick={() => (reapArmed ? reap() : armReap())}
            onblur={() => (reapArmed = false)}
            title="Delete this agent's worktree and branch — uncommitted work and its conversation go with them"
          >
            {reapArmed ? "Reap for good?" : "Reap"}
          </button>
        {/if}
      </div>
    {:else if drafting}
      <div class="head-left">
        <div class="prompt">New agent</div>
        <div class="meta">
          <span class="state state-draft">draft</span>
          <span class="target">in {projectName}</span>
        </div>
      </div>
      <div class="head-right">
        <button class="cancel" onclick={() => store.cancelDraft()}>Cancel</button>
      </div>
    {:else}
      <div class="head-left">
        <div class="prompt-empty">No agent selected</div>
      </div>
    {/if}
  </header>

  {#if store.selectedAgent && store.detailTab === "diff"}
    <AgentDiff />
  {:else}
    <div class="stream" bind:this={streamEl}>
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
      {:else if showRaw}
        {#each store.eventsForSelected as ev, i (i)}
          <pre class="raw">{JSON.stringify(ev.event, null, 2)}</pre>
        {/each}
      {:else}
        {#each store.eventsForSelected as ev, i (i)}
          {#each classify(ev) as k}
            {#if k.kind === "prompt"}
              <div class="block prompt-block">{k.text}</div>
            {:else if k.kind === "text"}
              <div class="block text"><Markdown text={k.text} /></div>
            {:else if k.kind === "tool_use"}
              <details class="block tool">
                <summary>→ {k.name} <span class="mono">{shortenInput(k.input)}</span></summary>
                <pre>{JSON.stringify(k.input, null, 2)}</pre>
              </details>
            {:else if k.kind === "tool_result"}
              <details class="block result">
                <summary>← result</summary>
                <pre>{toolResultText(k.content)}</pre>
              </details>
            {:else if k.kind === "thinking"}
              <details class="block thinking">
                <summary>thinking</summary>
                <div class="thinking-body"><Markdown text={k.text} /></div>
              </details>
            {:else if k.kind === "system"}
              <div class="block system">session: {k.subtype}</div>
            {:else if k.kind === "result"}
              <div class="block done" class:err={k.is_error}>
                {k.is_error ? "✗ error" : "✓ done"}
                {#if k.duration_ms != null}
                  — {formatDuration(k.duration_ms)}
                {/if}
                {#if k.output_tokens != null}
                  · {formatTokens(k.output_tokens)} tok
                {/if}
                {#if k.is_error && k.result}
                  — {k.result}
                {/if}
              </div>
            {:else}
              <details class="block raw-detail">
                <summary>event: {k.type ?? "unknown"}</summary>
                <pre>{JSON.stringify(ev.event, null, 2)}</pre>
              </details>
            {/if}
          {/each}
        {/each}
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
    border-bottom: 1px solid var(--border);
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem 1rem;
    min-height: 3.2rem;
  }

  .back {
    flex: none;
    width: 1.9rem;
    height: 1.9rem;
    padding: 0;
    border-radius: 5px;
    border: 1px solid var(--border);
    background: var(--surface);
    color: var(--fg);
    cursor: pointer;
    font-size: 0.95rem;
    line-height: 1;
  }

  .back:hover {
    border-color: var(--accent);
    color: var(--accent);
  }

  .head-left {
    /* Wide enough to be worth reading, or it wraps the controls to a new row. */
    flex: 1 1 14rem;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .prompt {
    font-size: 0.95rem;
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .prompt-empty {
    color: var(--fg-muted);
    font-style: italic;
  }

  .meta {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.3rem 0.5rem;
    font-size: 0.75rem;
    color: var(--fg-muted);
  }

  .meta .model {
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 0.05em 0.5em;
  }

  .live-stat {
    font-variant-numeric: tabular-nums;
    color: var(--fg-muted);
  }

  .meta code {
    max-width: 14rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .state {
    text-transform: uppercase;
    font-weight: 600;
    letter-spacing: 0.04em;
    font-size: 0.7rem;
    padding: 0.1rem 0.45rem;
    border-radius: 3px;
    background: var(--code-bg);
  }
  .state-running { color: #16a34a; }
  .state-completed { color: #2563eb; }
  .state-failed { color: #dc2626; }
  .state-orphaned { color: #d97706; }
  .state-stopped { color: #6b7280; }
  .state-draft { color: var(--accent); }

  .target {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .cancel:hover { border-color: var(--accent); color: var(--accent); }

  code {
    font-family: ui-monospace, monospace;
    background: var(--code-bg);
    padding: 0.05em 0.35em;
    border-radius: 3px;
  }

  .head-right {
    display: flex;
    flex: none;
    align-items: center;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 0.4rem 0.5rem;
    margin-left: auto;
  }

  .raw-toggle {
    font-size: 0.8rem;
    color: var(--fg-muted);
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
  }

  .head-right button {
    flex: none;
    white-space: nowrap;
    border-radius: 5px;
    border: 1px solid var(--border);
    background: var(--surface);
    padding: 0.35rem 0.85rem;
    font-size: 0.82rem;
    font-family: inherit;
    color: var(--fg);
    cursor: pointer;
  }

  .tabs {
    display: inline-flex;
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
  }

  .tabs button {
    border: none;
    border-radius: 0;
    background: transparent;
    padding: 0.32rem 0.8rem;
    font-size: 0.82rem;
    color: var(--fg-muted);
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
  }

  .tabs button:hover { color: var(--fg); }

  .tabs button.active {
    background: var(--selected);
    color: var(--fg);
    font-weight: 500;
  }

  .dirty-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: #f59e0b;
  }

  .stop:hover { border-color: #ef4444; color: #ef4444; }
  .reap:hover { border-color: var(--accent); color: var(--accent); }

  .reap.armed,
  .reap.armed:hover {
    border-color: #ef4444;
    background: #ef4444;
    color: #fff;
    font-weight: 500;
  }

  .stream {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
    padding: 1rem var(--pad-x) 2rem;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .hint {
    color: var(--fg-muted);
    font-style: italic;
    text-align: center;
    padding: 3rem 1rem;
    font-size: 0.9rem;
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
    margin-top: 0.4rem;
    font-size: 0.82rem;
    max-width: var(--measure);
    margin-left: auto;
    margin-right: auto;
  }

  .block {
    font-size: 0.9rem;
    line-height: 1.55;
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
    border-radius: 0 6px 6px 0;
    padding: 0.45rem 0.7rem;
    margin: 0.45rem 0 0.2rem;
    font-size: 0.88rem;
  }

  details.block {
    background: var(--panel-bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.4rem 0.75rem;
  }

  details.block summary {
    cursor: pointer;
    font-family: ui-monospace, monospace;
    font-size: 0.82rem;
    color: var(--fg-muted);
    overflow-wrap: anywhere;
  }

  details.block[open] summary { margin-bottom: 0.4rem; }

  details.block pre {
    background: transparent;
    margin: 0;
    padding: 0;
    font-size: 0.8rem;
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
    font-size: 0.86rem;
    color: var(--fg-muted);
    max-height: min(60vh, 34rem);
    overflow: auto;
  }

  .mono {
    font-family: ui-monospace, monospace;
    color: var(--fg-muted);
  }

  .system, .done {
    color: var(--fg-muted);
    font-family: ui-monospace, monospace;
    font-size: 0.8rem;
    padding: 0.15rem 0;
    overflow-wrap: anywhere;
    max-width: min(100%, var(--measure));
  }

  .done.err { color: #dc2626; }

  pre.raw {
    background: var(--panel-bg);
    border: 1px solid var(--border);
    padding: 0.5rem 0.75rem;
    border-radius: 5px;
    font-size: 0.78rem;
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: min(60vh, 34rem);
    overflow: auto;
  }

  .fail-banner {
    background: rgba(239, 68, 68, 0.08);
    border: 1px solid rgba(239, 68, 68, 0.3);
    border-radius: 6px;
    padding: 0.55rem 0.85rem;
    font-size: 0.85rem;
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem 0.6rem;
    align-items: baseline;
  }

  .fail-banner .label {
    color: #dc2626;
    font-weight: 600;
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .fail-banner .reason {
    color: var(--fg);
    font-family: ui-monospace, monospace;
    font-size: 0.82rem;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    min-width: 0;
  }
</style>
