<script lang="ts">
  import CodeSpans from "./CodeSpans.svelte";
  import GrowingLines from "./GrowingLines.svelte";
  import { highlightLines } from "./highlight.svelte";

  /**
   * Lines of a file beside their line numbers, as Read hands them back. The
   * numbers sit in a gutter of their own — out of the way of the code, and
   * out of a copied selection. Goes inside the caller's `<pre>`, which they
   * fill a chunk at a time as it scrolls.
   */
  let { lines, lang }: { lines: { n: string; text: string }[]; lang: string } = $props();

  const spans = $derived(highlightLines(lines.map((l) => l.text), lang));

  /** The widest number, which the gutter is kept wide enough for before it arrives. */
  const digits = $derived(lines.reduce((w, l) => Math.max(w, l.n.length), 0));
</script>

<span class="numbered" style:--digits={digits}
  ><GrowingLines count={lines.length}
    >{#snippet children(shown: number)}{#each lines.slice(0, shown) as l, i}<span class="n"
          >{l.n}</span
        ><code>{#if spans?.[i]}<CodeSpans spans={spans[i]} />{:else}{l.text}{/if}</code
        >{/each}{/snippet}</GrowingLines
  ></span
>

<style>
  /* A long line wraps under itself, not under its number. */
  .numbered {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    column-gap: 0.9em;
  }

  .n {
    min-width: calc(var(--digits) * 1ch);
    text-align: right;
    color: var(--fg-muted);
    opacity: 0.6;
    user-select: none;
  }
</style>
