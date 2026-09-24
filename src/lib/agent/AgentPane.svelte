<script lang="ts">
  /**
   * The detail pane: the selected agent with its header, then either its diff
   * or its transcript and composer — or the blank page for a new one.
   */
  import { store } from "$lib/state/store.svelte";
  import { hosts } from "$lib/state/hosts.svelte";
  import AgentHeader from "./AgentHeader.svelte";
  import AgentDiff from "./AgentDiff.svelte";
  import AgentComposer from "./AgentComposer.svelte";
  import Transcript from "$lib/transcript/Transcript.svelte";

  /**
   * The pane's own height. The composer's growing parts cap against it rather
   * than the window: the title bar, banners and header above have already
   * spent some of the window, and caps in `vh` promised the composer space
   * that wasn't there — so on a short window it ran off the bottom.
   */
  let height = $state(0);

  /** Why the selected agent's Host can't be reached, if it can't. */
  const away = $derived(store.selectedAgent ? hosts.problem(store.selectedAgent.host) : null);
</script>

<section bind:clientHeight={height} style:--pane-h={height ? `${height}px` : null}>
  <!-- A draft has no header: its Cancel lives in the window bar. -->
  {#if store.selectedAgent}
    <AgentHeader />
    {#if away}
      <!-- Its Host is the only record of it, so this is as it was last seen,
           and nothing can be done to it until the Host is back. -->
      <div class="away" role="status">{away}: showing this agent as it was last seen.</div>
    {/if}
  {/if}

  {#if store.selectedAgent && store.detailTab === "diff"}
    <AgentDiff />
  {:else}
    <Transcript />
    <AgentComposer />
  {/if}
</section>

<style>
  .away {
    background: var(--warning-soft-bg);
    border-bottom: 1px solid var(--warning-soft-border);
    padding: 0.45rem var(--pad-x);
    font-size: var(--text-sm);
  }

  section {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    background: var(--surface);
    overflow-x: hidden;
    /* Only once even the smallest composer doesn't fit beside the header —
       the transcript gives up all its height first. Scrolling beats cutting
       the input off. */
    overflow-y: auto;
  }
</style>
