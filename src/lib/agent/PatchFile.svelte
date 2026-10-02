<script lang="ts">
  import CodeSpans from "$lib/code/CodeSpans.svelte";
  import GrowingLines from "$lib/code/GrowingLines.svelte";
  import { CHUNK, whenNear } from "$lib/code/growing";
  import { languageOf } from "$lib/code/highlight.svelte";
  import { highlightPatch, type PatchFile } from "./patch";

  /**
   * One file of the Diff tab's patch. A big diff is thousands of lines over
   * dozens of files, so a file is only coloured and drawn once it scrolls
   * near, and then a chunk at a time as its own box scrolls.
   */
  let { file }: { file: PatchFile } = $props();

  /** The file has come within reach of the screen. It stays drawn after. */
  let near = $state(false);

  const highlighted = $derived(near ? highlightPatch(file.lines, languageOf(file.path)) : []);
</script>

<details class="patch-file" open {@attach whenNear(() => (near = true))}>
  <summary>{file.path}</summary>
  <div class="patch">
    {#if near}
      <GrowingLines count={file.lines.length}>
        {#snippet children(shown: number)}
          {#each file.lines.slice(0, shown) as line, i (i)}
            {@const spans = highlighted[i]}
            <!-- Coloured, a line keeps its +/- and its tint to say what
                 changed, and the code takes its syntax colours. -->
            <div class={`line ${line.kind}`} class:coloured={!!spans}
              >{#if spans}<span class="sign">{line.text[0]}</span><CodeSpans
                  {spans}
                />{:else}{line.text || " "}{/if}</div
            >
          {/each}
        {/snippet}
      </GrowingLines>
    {:else}
      <!-- As tall as the lines it stands in for, so the files below don't
           move when they arrive. -->
      <div class="unseen" style:--lines={Math.min(file.lines.length, CHUNK)}></div>
    {/if}
  </div>
</details>

<style>
  .patch-file {
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    min-width: 0;
  }

  .patch-file summary {
    cursor: pointer;
    padding: 0.4rem 0.7rem;
    background: var(--float-bg, var(--panel-bg));
    -webkit-backdrop-filter: var(--float-filter, none);
    backdrop-filter: var(--float-filter, none);
    font-family: var(--font-mono);
    font-size: var(--text-md);
    color: var(--fg-muted);
    overflow-wrap: anywhere;
    border-radius: var(--radius-sm) var(--radius-sm) 0 0;
    /* Stays put while its own patch scrolls past underneath. */
    position: sticky;
    top: 0;
    z-index: var(--z-base);
  }

  .patch-file[open] summary {
    border-bottom: 1px solid var(--border);
  }

  .patch {
    /* Code lines keep their shape: this scrolls both ways on its own, so one
       big file can't swallow the pane. */
    overflow: auto;
    overscroll-behavior: contain;
    max-height: clamp(10rem, 50vh, 32rem);
    border-radius: 0 0 var(--radius-sm) var(--radius-sm);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    line-height: var(--leading-relaxed);
  }

  .unseen {
    height: calc(var(--lines) * 1lh);
  }

  .line {
    padding: 0 0.7rem;
    white-space: pre;
    min-width: max-content;
  }

  .line.add {
    background: var(--diff-add-bg);
    color: var(--diff-add-fg);
  }

  .line.del {
    background: var(--diff-del-bg);
    color: var(--diff-del-fg);
  }

  .line.hunk {
    background: var(--code-bg);
    color: var(--fg-muted);
  }

  .line.coloured {
    color: var(--fg);
  }

  .line.add .sign { color: var(--diff-add-fg); }
  .line.del .sign { color: var(--diff-del-fg); }
</style>
