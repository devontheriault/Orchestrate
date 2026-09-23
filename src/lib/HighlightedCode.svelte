<script lang="ts">
  import CodeSpans from "./CodeSpans.svelte";
  import { highlightLines } from "./highlight.svelte";

  /**
   * Code coloured by its syntax. The `<pre>` around it is the caller's, so
   * each place code appears keeps its own wrapping and scrolling. Unknown
   * languages, and everything before the highlighter loads, render plain.
   */
  let { code, lang }: { code: string; lang: string } = $props();

  const lines = $derived(highlightLines(code.split("\n"), lang));
</script>

<code
  >{#if lines}{#each lines as spans, i}{#if i > 0}{"\n"}{/if}<CodeSpans
        {spans}
      />{/each}{:else}{code}{/if}</code
>
