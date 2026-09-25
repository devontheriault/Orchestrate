<script lang="ts">
  import { store } from "$lib/state/store.svelte";
  import { formatDuration, formatTokens } from "$lib/format";
  import { turnOutputTokens } from "./turnTokens";

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

  const outputTokens = $derived(turnOutputTokens(store.eventsForSelected));
</script>

<!-- Sits directly on top of the input box: while the agent works, how long
     it's been and how much it's produced is what the eye looks for next. -->
<div class="turn-stats" aria-live="off">
  <span class="status-dot status-running" aria-hidden="true"></span>
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

  .sep {
    opacity: 0.6;
  }
</style>
