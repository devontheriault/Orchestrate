<script lang="ts">
  import { store } from "./store.svelte";
  import { formatDuration, formatTokens } from "./format";

  // Ticks once a second while the agent runs, so the elapsed timer advances
  // without waiting for the next stream event.
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

  const elapsedMs = $derived(turnStartedAt != null ? Math.max(0, now - turnStartedAt) : 0);

  /**
   * Tokens generated so far in the running turn: `claude` reports usage per
   * API call rather than as incremental deltas, so this sums each call's
   * output as it arrives — an estimate that climbs as the turn progresses,
   * not the exact total the final `result` event reports.
   */
  const outputTokens = $derived.by(() => {
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
</script>

<!-- Sits directly on top of the input box: while the agent works, how long
     it's been and how much it's produced is what the eye looks for next. -->
<div class="turn-stats" aria-live="off">
  <span class="pulse" aria-hidden="true"></span>
  <span title="Time since the agent started working">{formatDuration(elapsedMs)}</span>
  <span class="sep" aria-hidden="true">·</span>
  <span title="Tokens generated since the agent started working (estimate)">
    ~{formatTokens(outputTokens)} tok
  </span>
</div>

<style>
  .turn-stats {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: 0 0.3rem;
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
    color: var(--fg-muted);
  }

  /* The same green the header uses for "running" — the row is only ever on
     while the agent works, so it doubles as the live indicator. */
  .pulse {
    width: 0.4rem;
    height: 0.4rem;
    border-radius: var(--radius-pill);
    background: var(--success);
    animation: pulse 1.6s ease-in-out infinite;
  }

  .sep {
    opacity: 0.6;
  }

  @keyframes pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.3;
    }
  }
</style>
