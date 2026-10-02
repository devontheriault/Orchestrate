<script lang="ts">
  import MarkdownBlocks from "./MarkdownBlocks.svelte";
  import MarkdownInline from "./MarkdownInline.svelte";
  import HighlightedCode from "$lib/code/HighlightedCode.svelte";
  import Copyable from "$lib/code/Copyable.svelte";
  import type { Token } from "./markdown";

  /** Block-level tokens. `Markdown.svelte` owns the lexing and the styling. */
  let { tokens }: { tokens?: Token[] } = $props();

  /** ```ts title=x ``` names the language first; the rest is for tooling. */
  function language(lang: string | undefined): string {
    return (lang ?? "").trim().split(/\s+/)[0] ?? "";
  }
</script>

{#each tokens ?? [] as block}
  {@const t = block as any}
  {#if t.type === "paragraph"}
    <p><MarkdownInline tokens={t.tokens} /></p>
  {:else if t.type === "text"}
    <!-- A tight list item's content: inline, with no paragraph around it. -->
    {#if t.tokens?.length}
      <MarkdownInline tokens={t.tokens} />
    {:else}{t.text ?? ""}{/if}
  {:else if t.type === "heading"}
    <svelte:element this={`h${Math.min(6, Math.max(1, t.depth ?? 1))}`}>
      <MarkdownInline tokens={t.tokens} />
    </svelte:element>
  {:else if t.type === "code"}
    {@const lang = language(t.lang)}
    <Copyable text={t.text ?? ""} class={lang ? "codeblock labelled" : "codeblock"}>
      {#if lang}<span class="lang">{lang}</span>{/if}<pre><HighlightedCode
          code={t.text ?? ""}
          {lang}
        /></pre>
    </Copyable>
  {:else if t.type === "list"}
    <svelte:element
      this={t.ordered ? "ol" : "ul"}
      start={t.ordered && t.start !== 1 && t.start !== "" ? t.start : undefined}
    >
      {#each t.items ?? [] as item}
        <li class:task={item.task}><MarkdownBlocks tokens={item.tokens} /></li>
      {/each}
    </svelte:element>
  {:else if t.type === "checkbox"}
    <input type="checkbox" checked={t.checked} disabled />
  {:else if t.type === "blockquote"}
    <blockquote><MarkdownBlocks tokens={t.tokens} /></blockquote>
  {:else if t.type === "table"}
    <div class="table-wrap">
      <table>
        <thead>
          <tr>
            {#each t.header ?? [] as cell, i}
              <th style={t.align?.[i] ? `text-align: ${t.align[i]}` : undefined}>
                <MarkdownInline tokens={cell.tokens} />
              </th>
            {/each}
          </tr>
        </thead>
        <tbody>
          {#each t.rows ?? [] as row}
            <tr>
              {#each row as cell, i}
                <td style={t.align?.[i] ? `text-align: ${t.align[i]}` : undefined}>
                  <MarkdownInline tokens={cell.tokens} />
                </td>
              {/each}
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else if t.type === "hr"}
    <hr />
  {:else if t.type === "space" || t.type === "def"}
    <!-- Blank lines and link definitions have nothing to show. -->
  {:else if t.type === "html"}
    <!-- Raw HTML: shown as the source it is, never parsed. -->
    <div class="html-literal">{t.raw ?? t.text ?? ""}</div>
  {:else}
    <!-- Anything marked grows later still reaches the reader, as its source. -->
    <p>{t.raw ?? t.text ?? ""}</p>
  {/if}
{/each}
