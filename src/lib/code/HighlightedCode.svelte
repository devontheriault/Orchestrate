<script lang="ts">
  import CodeSpans from "./CodeSpans.svelte";
  import GrowingLines from "./GrowingLines.svelte";
  import { highlightLines } from "./highlight.svelte";

  /**
   * Code coloured by its syntax. The `<pre>` around it is the caller's, so
   * each place code appears keeps its own wrapping and scrolling; a long
   * piece fills it a chunk at a time as it scrolls. Unknown languages, and
   * everything before the highlighter loads, render plain.
   */
  let { code, lang }: { code: string; lang: string } = $props();

  const plain = $derived(code.split("\n"));
  const lines = $derived(highlightLines(plain, lang));
</script>

<code
  ><GrowingLines count={plain.length}
    >{#snippet children(shown: number)}{#each plain.slice(0, shown) as text, i}{#if i > 0}{"\n"}{/if}{#if lines?.[i]}<CodeSpans
            spans={lines[i]}
          />{:else}{text}{/if}{/each}{/snippet}</GrowingLines
  ></code
>
