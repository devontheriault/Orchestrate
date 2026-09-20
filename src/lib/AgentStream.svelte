<script lang="ts">
  import { store } from "./store.svelte";
  import AgentDiff from "./AgentDiff.svelte";
  import AgentComposer from "./AgentComposer.svelte";
  import type { AgentEvent } from "./api";

  /** Present only when the agent list is off-screen, on a narrow window. */
  let { onBack }: { onBack?: () => void } = $props();

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
    | { kind: "result"; result: string; is_error: boolean }
    | { kind: "raw"; type?: string };

  function classify(ev: AgentEvent): Kind[] {
    const e = ev.event as { type?: string; message?: any; subtype?: string; result?: string; is_error?: boolean; prompt?: string } | null;
    if (!e || typeof e !== "object") return [{ kind: "raw" }];
    // Our own event, not claude's: the prompt the user sent for this turn.
    if (e.type === "cw_prompt") return [{ kind: "prompt", text: e.prompt ?? "" }];
    if (e.type === "system") return [{ kind: "system", subtype: e.subtype ?? "?" }];
    if (e.type === "result")
      return [{ kind: "result", result: e.result ?? "", is_error: !!e.is_error }];
    if ((e.type === "assistant" || e.type === "user") && e.message?.content) {
      const blocks = Array.isArray(e.message.content) ? e.message.content : [];
      return blocks.map((b: any): Kind => {
        if (b.type === "text") return { kind: "text", text: b.text ?? "" };
        if (b.type === "tool_use")
          return { kind: "tool_use", name: b.name ?? "?", input: b.input, id: b.id ?? "" };
        if (b.type === "tool_result")
          return { kind: "tool_result", tool_use_id: b.tool_use_id ?? "", content: b.content };
        if (b.type === "thinking") return { kind: "thinking", text: b.thinking ?? "" };
        return { kind: "raw", type: b.type };
      });
    }
    return [{ kind: "raw", type: e.type }];
  }

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
      <button class="back" onclick={onBack} title="Back to agents" aria-label="Back to agents"
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

      {#if !store.selectedAgent}
        <div class="hint">Select an agent to see its output.</div>
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
              <div class="block text">{k.text}</div>
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
                <pre>{k.text}</pre>
              </details>
            {:else if k.kind === "system"}
              <div class="block system">session: {k.subtype}</div>
            {:else if k.kind === "result"}
              <div class="block done" class:err={k.is_error}>
                {k.is_error ? "✗ error" : "✓ done"} — {k.result}
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

  .block {
    font-size: 0.9rem;
    line-height: 1.55;
    min-width: 0;
  }

  .text {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
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
