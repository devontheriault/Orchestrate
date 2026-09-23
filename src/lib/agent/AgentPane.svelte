<script lang="ts">
  /**
   * The detail pane: the selected agent — or the blank page for a new one —
   * with its header, then either its diff or its transcript and composer.
   */
  import { store } from "$lib/state/store.svelte";
  import AgentHeader from "./AgentHeader.svelte";
  import AgentDiff from "./AgentDiff.svelte";
  import AgentComposer from "./AgentComposer.svelte";
  import Transcript from "$lib/transcript/Transcript.svelte";

  /** The blank page for an agent that hasn't been spawned yet. */
  const drafting = $derived(store.drafting && !store.selectedAgent);
</script>

<section>
  {#if store.selectedAgent || drafting}
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
    overflow: hidden;
  }
</style>
