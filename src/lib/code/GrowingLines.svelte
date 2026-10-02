<script lang="ts">
  import type { Snippet } from "svelte";
  import { CHUNK, whenNear } from "./growing";

  /**
   * The first `CHUNK` of `count` lines, and another `CHUNK` each time the
   * reader scrolls near the end of those shown. `children` draws as many as
   * it is given; the box they scroll in is the caller's, often a `<pre>` —
   * so nothing here may leave whitespace between the tags.
   *
   * The marker below the last line is keyed on the count, so it is watched
   * afresh under each new chunk: still in reach after one, it asks for the
   * next.
   */
  let { count, children }: { count: number; children: Snippet<[number]> } = $props();

  let shown = $state(CHUNK);
</script>

{@render children(Math.min(shown, count))}{#if shown < count}{#key shown}<span
      class="more"
      aria-hidden="true"
      {@attach whenNear(() => (shown += CHUNK))}
    ></span>{/key}{/if}

<style>
  /* Nothing to see, below the last line shown — across the whole row of a
     grid it sits in. */
  .more {
    display: block;
    height: 1px;
    grid-column: 1 / -1;
  }
</style>
