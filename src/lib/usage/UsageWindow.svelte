<script lang="ts">
  import { store } from "$lib/state/store.svelte";
  import { usage, REFRESH_MS } from "./usage.svelte";
  import type { ModelUsage } from "$lib/api";

  /** Claude Code's window names, in the words the user sees them in. */
  const WINDOW_NAMES: Record<string, { title: string; sub: string }> = {
    five_hour: { title: "Current session", sub: "5-hour window" },
    seven_day: { title: "Current week", sub: "all models" },
    seven_day_opus: { title: "Current week", sub: "Opus" },
  };

  function windowName(kind: string) {
    return WINDOW_NAMES[kind] ?? { title: kind.replace(/_/g, " "), sub: "" };
  }

  // Ticks while the window is up, so a countdown to the next reset advances
  // on its own. The window only exists while open, so no guard is needed.
  let now = $state(Date.now());
  $effect(() => {
    const id = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(id);
  });

  // A running agent spends while the user watches; re-read on a timer so the
  // numbers in front of them are the current ones.
  $effect(() => {
    const id = setInterval(() => usage.load(), REFRESH_MS);
    return () => clearInterval(id);
  });

  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") usage.close();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const limits = $derived(usage.summary?.limits ?? null);
  const agents = $derived(usage.summary?.agents ?? []);

  const selectedUsage = $derived(
    agents.find((a) => a.agent_id === store.selectedAgentId) ?? null,
  );

  /** Add up per-model rows from any number of agents, costliest first. */
  function merge(rows: ModelUsage[][]): ModelUsage[] {
    const by = new Map<string, ModelUsage>();
    for (const list of rows) {
      for (const m of list) {
        const at = by.get(m.model);
        if (!at) {
          by.set(m.model, { ...m });
          continue;
        }
        at.input_tokens += m.input_tokens;
        at.output_tokens += m.output_tokens;
        at.cache_read_tokens += m.cache_read_tokens;
        at.cache_creation_tokens += m.cache_creation_tokens;
        at.cost_usd += m.cost_usd;
      }
    }
    return [...by.values()].sort((a, b) => b.cost_usd - a.cost_usd);
  }

  const models = $derived(
    usage.scope === "agent"
      ? selectedUsage?.models ?? []
      : merge(agents.map((a) => a.models)),
  );

  const totals = $derived(
    models.reduce(
      (acc, m) => ({
        input: acc.input + m.input_tokens,
        output: acc.output + m.output_tokens,
        cacheRead: acc.cacheRead + m.cache_read_tokens,
        cacheWrite: acc.cacheWrite + m.cache_creation_tokens,
        cost: acc.cost + m.cost_usd,
      }),
      { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, cost: 0 },
    ),
  );

  /** Answers the agent has finished — a Turn each, in the domain's terms. */
  const replies = $derived(
    usage.scope === "agent"
      ? selectedUsage?.turns ?? 0
      : agents.reduce((n, a) => n + a.turns, 0),
  );

  function agentCost(models: ModelUsage[]): number {
    return models.reduce((n, m) => n + m.cost_usd, 0);
  }

  function agentTokens(models: ModelUsage[]): number {
    return models.reduce(
      (n, m) => n + m.input_tokens + m.output_tokens + m.cache_read_tokens,
      0,
    );
  }

  /**
   * What to call an agent in the by-agent list. Logs outlive the Agents that
   * wrote them, so an id with no Agent behind it is one that was Reaped —
   * worth counting, and worth saying so.
   */
  function agentLabel(id: string): { text: string; reaped: boolean } {
    const agent = store.agents.find((a) => a.id === id);
    if (!agent) return { text: "reaped agent", reaped: true };
    const name = store.agentName(agent).trim();
    return { text: name || agent.branch, reaped: false };
  }

  /** Jump to an agent's output and get out of the way. */
  function jumpTo(id: string) {
    if (store.showAgent(id)) usage.close();
  }

  function formatTokens(n: number): string {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(n >= 10_000_000 ? 0 : 1)}M`;
    if (n >= 1000) return `${(n / 1000).toFixed(n >= 10_000 ? 0 : 1)}k`;
    return `${n}`;
  }

  function formatCost(usd: number): string {
    if (usd === 0) return "$0";
    if (usd < 0.01) return "<$0.01";
    if (usd >= 100) return `$${Math.round(usd)}`;
    return `$${usd.toFixed(2)}`;
  }

  function formatPercent(fraction: number): string {
    const pct = fraction * 100;
    if (pct > 0 && pct < 1) return "<1%";
    return `${Math.round(pct)}%`;
  }

  /** "in 1h 12m" — how long this window has left before it rolls over. */
  function untilReset(unixSeconds: number): string {
    const ms = unixSeconds * 1000 - now;
    if (ms <= 0) return "resetting now";
    const minutes = Math.round(ms / 60000);
    if (minutes < 60) return `in ${minutes}m`;
    const hours = Math.floor(minutes / 60);
    if (hours < 24) return `in ${hours}h ${minutes % 60}m`;
    return `in ${Math.floor(hours / 24)}d ${hours % 24}h`;
  }

  /** The reset moment itself: a clock time today, a weekday further out. */
  function resetAt(unixSeconds: number): string {
    const at = new Date(unixSeconds * 1000);
    const sameDay = at.toDateString() === new Date(now).toDateString();
    return at.toLocaleString([], {
      hour: "numeric",
      minute: "2-digit",
      ...(sameDay ? {} : { weekday: "short" }),
    });
  }

  function clockTime(iso: string): string {
    return new Date(iso).toLocaleTimeString([], {
      hour: "numeric",
      minute: "2-digit",
    });
  }
</script>

<!-- Closes on a click outside the dialog, not on one that lands inside it. -->
<div
  class="backdrop"
  role="presentation"
  onclick={(e) => {
    if (e.target === e.currentTarget) usage.close();
  }}
>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="usage-title" tabindex="-1">
    <header>
      <h2 id="usage-title">Token usage</h2>
      {#if usage.loading && !usage.summary}
        <span class="as-of">reading logs…</span>
      {:else if limits}
        <span class="as-of" title="Claude Code reports limits as an agent works">
          limits as of {clockTime(limits.observed_at)}
        </span>
      {/if}
      <button class="close" onclick={() => usage.close()} aria-label="Close">×</button>
    </header>

    <div class="body">
      {#if usage.error}
        <div class="err">{usage.error}</div>
      {/if}

      <section>
        <div class="section-label">Rate limits</div>
        {#if limits}
          {#each limits.windows as w (w.kind)}
            {@const name = windowName(w.kind)}
            <div class="limit">
              <div class="limit-head">
                <span class="name">{name.title}</span>
                {#if name.sub}<span class="sub">{name.sub}</span>{/if}
                <span class="pct" class:warn={w.utilization >= 0.75} class:hot={w.utilization >= 0.9}>
                  {formatPercent(w.utilization)}
                </span>
              </div>
              <div
                class="bar"
                role="meter"
                aria-valuenow={Math.round(w.utilization * 100)}
                aria-valuemin="0"
                aria-valuemax="100"
                aria-label={`${name.title} usage`}
              >
                <div
                  class="fill"
                  class:warn={w.utilization >= 0.75}
                  class:hot={w.utilization >= 0.9}
                  style={`width: ${Math.min(100, Math.max(0, w.utilization * 100))}%`}
                ></div>
              </div>
              <div class="limit-foot">
                resets {untilReset(w.resets_at)} · {resetAt(w.resets_at)}
              </div>
            </div>
          {/each}
          {#if limits.using_overage}
            <div class="note">Running on extra usage beyond the included limit.</div>
          {:else if limits.status && limits.status !== "allowed"}
            <div class="note">Claude Code reports this account as {limits.status}.</div>
          {/if}
        {:else}
          <div class="empty">
            No limit report yet — Claude Code sends one while an agent works.
          </div>
        {/if}
      </section>

      <section>
        <div class="section-label">
          Tokens
          <div class="scope" role="group" aria-label="Whose tokens to count">
            <button class:active={usage.scope === "all"} onclick={() => (usage.scope = "all")}>
              All agents
            </button>
            <button
              class:active={usage.scope === "agent"}
              disabled={!store.selectedAgentId}
              onclick={() => (usage.scope = "agent")}
            >
              This agent
            </button>
          </div>
        </div>

        {#if models.length === 0}
          <div class="empty">
            {usage.scope === "agent"
              ? store.selectedAgentId
                ? "This agent hasn't finished any work yet."
                : "No agent selected."
              : "No agent has finished any work yet."}
          </div>
        {:else}
          <div class="totals">
            <div class="stat">
              <span class="v">{formatTokens(totals.input)}</span>
              <span class="k">input</span>
            </div>
            <div class="stat">
              <span class="v">{formatTokens(totals.output)}</span>
              <span class="k">output</span>
            </div>
            <div class="stat">
              <span class="v">{formatTokens(totals.cacheRead)}</span>
              <span class="k">cache read</span>
            </div>
            <div class="stat">
              <span class="v">{formatTokens(totals.cacheWrite)}</span>
              <span class="k">cache write</span>
            </div>
            <div class="stat cost">
              <span class="v">{formatCost(totals.cost)}</span>
              <span class="k">{replies} repl{replies === 1 ? "y" : "ies"}</span>
            </div>
          </div>

          <div class="table-scroll">
            <table>
              <thead>
                <tr>
                  <th>Model</th>
                  <th>In</th>
                  <th>Out</th>
                  <th>Cache r/w</th>
                  <th>Cost</th>
                </tr>
              </thead>
              <tbody>
                {#each models as m (m.model)}
                  <tr>
                    <td class="model" title={m.model}>{store.modelName(m.model)}</td>
                    <td>{formatTokens(m.input_tokens)}</td>
                    <td>{formatTokens(m.output_tokens)}</td>
                    <td class="muted">
                      {formatTokens(m.cache_read_tokens)} / {formatTokens(
                        m.cache_creation_tokens,
                      )}
                    </td>
                    <td>{formatCost(m.cost_usd)}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/if}
      </section>

      {#if usage.scope === "all" && agents.length > 0}
        <section>
          <div class="section-label">By agent</div>
          <div class="agent-list">
            {#each agents as a (a.agent_id)}
              {@const label = agentLabel(a.agent_id)}
              <button
                class="agent-row"
                class:selected={a.agent_id === store.selectedAgentId}
                disabled={label.reaped}
                onclick={() => jumpTo(a.agent_id)}
                title={label.reaped ? "This agent has been reaped" : "Show this agent"}
              >
                <code class="id">{a.agent_id}</code>
                <span class="task" class:reaped={label.reaped}>{label.text}</span>
                <span class="tokens">{formatTokens(agentTokens(a.models))}</span>
                <span class="money">{formatCost(agentCost(a.models))}</span>
              </button>
            {/each}
          </div>
        </section>
      {/if}
    </div>

    <footer>
      <span class="hint">Ctrl+Shift+U or Esc to close</span>
      <button onclick={() => usage.refresh()} disabled={usage.refreshing}>
        {usage.refreshing ? "Refreshing…" : "Refresh"}
      </button>
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: var(--scrim);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: var(--space-6);
    z-index: var(--z-overlay);
    backdrop-filter: blur(2px);
  }

  .dialog {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    width: min(36rem, 100%);
    max-height: 100%;
    box-shadow: var(--shadow-modal);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  header {
    display: flex;
    align-items: baseline;
    gap: 0.25rem 0.75rem;
    padding: 1rem 1.25rem 0.75rem;
    border-bottom: 1px solid var(--border);
  }

  h2 {
    font-size: var(--text-2xl);
    margin: 0;
    font-weight: var(--weight-semibold);
  }

  .as-of {
    font-size: var(--text-xs);
    color: var(--fg-muted);
  }

  .close {
    margin-left: auto;
    align-self: center;
    background: transparent;
    border: none;
    color: var(--fg-muted);
    font-size: var(--text-3xl);
    line-height: var(--leading-none);
    padding: 0 0.25rem;
    cursor: pointer;
  }

  .close:hover {
    color: var(--fg);
  }

  /* The window is a fixed frame; everything between head and foot scrolls. */
  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0.9rem 1.25rem 1.1rem;
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
  }

  .section-label {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem 0.75rem;
    font-size: var(--text-xs);
    color: var(--fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    margin-bottom: var(--space-4);
  }

  .limit {
    margin-bottom: var(--space-5);
  }

  .limit:last-child {
    margin-bottom: 0;
  }

  .limit-head {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
    font-size: var(--text-lg);
  }

  .limit-head .name {
    font-weight: var(--weight-medium);
  }

  .limit-head .sub,
  .limit-foot {
    font-size: var(--text-xs);
    color: var(--fg-muted);
  }

  .limit-head .pct {
    margin-left: auto;
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    font-size: var(--text-md);
  }

  .pct.warn {
    color: var(--attention);
  }

  .pct.hot {
    color: var(--danger-text);
  }

  .bar {
    height: 0.5rem;
    border-radius: var(--radius-pill);
    background: var(--code-bg);
    overflow: hidden;
    margin: 0.3rem 0 0.25rem;
  }

  .fill {
    height: 100%;
    border-radius: var(--radius-pill);
    background: var(--accent);
    transition: width var(--duration-slow) var(--ease);
  }

  .fill.warn {
    background: var(--attention);
  }

  .fill.hot {
    background: var(--danger);
  }

  .totals {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-4);
    margin-bottom: var(--space-4);
  }

  .stat {
    flex: 1 1 5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    padding: 0.4rem 0.55rem;
    background: var(--panel-bg);
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }

  .stat .v {
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    font-size: var(--text-xl);
  }

  .stat .k {
    font-size: var(--text-2xs);
    color: var(--fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .stat.cost .v {
    color: var(--accent);
    font-weight: var(--weight-semibold);
  }

  .scope {
    display: flex;
    margin-left: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }

  .scope button {
    background: var(--surface);
    text-transform: none;
    border: none;
    color: var(--fg-muted);
    font-size: var(--text-xs);
    letter-spacing: 0.02em;
    padding: 0.22rem 0.55rem;
    cursor: pointer;
  }

  .scope button:hover:not(:disabled):not(.active) {
    background: var(--hover);
  }

  .scope button.active {
    background: var(--selected);
    color: var(--fg);
  }

  .scope button:disabled {
    opacity: 0.45;
    cursor: default;
  }

  /* A model name can be long; the numbers keep their columns either way. */
  .table-scroll {
    overflow-x: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }

  th {
    text-align: right;
    font-weight: var(--weight-medium);
    font-size: var(--text-2xs);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--fg-muted);
    padding: 0.35rem 0.6rem;
    background: var(--panel-bg);
    white-space: nowrap;
  }

  th:first-child {
    text-align: left;
  }

  td {
    padding: 0.35rem 0.6rem;
    text-align: right;
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    border-top: 1px solid var(--border);
  }

  td.model {
    text-align: left;
    font-family: inherit;
    max-width: 12rem;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  td.muted {
    color: var(--fg-muted);
  }

  .agent-list {
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    max-height: 12rem;
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  .agent-row {
    width: 100%;
    display: flex;
    align-items: baseline;
    gap: var(--space-4);
    padding: 0.35rem 0.6rem;
    background: transparent;
    border: none;
    border-top: 1px solid var(--border);
    color: var(--fg);
    font-size: var(--text-sm);
    text-align: left;
    cursor: pointer;
  }

  .agent-row:first-child {
    border-top: none;
  }

  .agent-row:hover:not(:disabled) {
    background: var(--hover);
  }

  .agent-row.selected {
    background: var(--selected);
  }

  .agent-row:disabled {
    cursor: default;
  }

  .agent-row .id {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--fg-muted);
    background: transparent;
    flex: none;
  }

  .agent-row .task {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .agent-row .task.reaped {
    color: var(--fg-muted);
    font-style: italic;
  }

  .agent-row .tokens,
  .agent-row .money {
    flex: none;
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    font-size: var(--text-xs);
  }

  .agent-row .tokens {
    color: var(--fg-muted);
  }

  .empty,
  .note {
    font-size: var(--text-md);
    color: var(--fg-muted);
  }

  .note {
    margin-top: var(--space-4);
    color: var(--attention);
  }

  .err {
    background: var(--danger-soft-bg);
    border: 1px solid var(--danger-soft-border);
    border-radius: var(--radius-md);
    padding: 0.5rem 0.7rem;
    font-size: var(--text-sm);
    font-family: var(--font-mono);
    overflow-wrap: anywhere;
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-5);
    padding: 0.6rem 1.25rem 0.75rem;
    border-top: 1px solid var(--border);
  }

  .hint {
    font-size: var(--text-xs);
    color: var(--fg-muted);
  }

  footer button {
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    background: var(--surface);
    color: var(--fg);
    padding: 0.28rem 0.7rem;
    font-size: var(--text-sm);
    cursor: pointer;
  }

  footer button:hover:not(:disabled) {
    border-color: var(--accent);
    color: var(--accent);
  }

  footer button:disabled {
    opacity: 0.6;
    cursor: default;
  }
</style>
