<script lang="ts">
  /**
   * The detail pane: the selected agent with its header, then either its diff
   * or its transcript and composer — or the blank page for a new one.
   */
  import { store } from "$lib/state/store.svelte";
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
</script>

<section bind:clientHeight={height} style:--pane-h={height ? `${height}px` : null}>
  <!-- A draft has no header: its Cancel lives in the window bar. -->
  {#if store.selectedAgent}
    <AgentHeader />
  {/if}

  {#if store.selectedAgent && store.detailTab === "diff"}
    <AgentDiff />
  {:else}
    <Transcript />
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
    overflow-x: hidden;
    /* Only once even the smallest composer doesn't fit beside the header —
       the transcript gives up all its height first. Scrolling beats cutting
       the input off. */
    overflow-y: auto;
  }
</style>
