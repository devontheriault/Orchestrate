<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import MarkdownInline from "./MarkdownInline.svelte";
  import { decodeEntities, safeHref, type Token } from "./markdown";

  let { tokens }: { tokens?: Token[] } = $props();

  /**
   * The transcript lives in the app's only window, so letting a link navigate
   * would replace the app with the page. Links go to the OS browser instead.
   */
  function open(e: MouseEvent, href: string) {
    e.preventDefault();
    openUrl(href).catch(() => {});
  }
</script>

{#each tokens ?? [] as token}
  {@const t = token as any}
  {#if t.type === "text"}
    {#if t.tokens?.length}
      <MarkdownInline tokens={t.tokens} />
    {:else}{decodeEntities(t.text ?? "")}{/if}
  {:else if t.type === "strong"}
    <strong><MarkdownInline tokens={t.tokens} /></strong>
  {:else if t.type === "em"}
    <em><MarkdownInline tokens={t.tokens} /></em>
  {:else if t.type === "del"}
    <del><MarkdownInline tokens={t.tokens} /></del>
  {:else if t.type === "codespan"}
    <code>{t.text ?? ""}</code>
  {:else if t.type === "br"}
    <br />
  {:else if t.type === "link"}
    {@const href = safeHref(t.href)}
    {#if href}
      <a
        {href}
        title={t.title || href}
        target="_blank"
        rel="noreferrer noopener"
        onclick={(e) => open(e, href)}><MarkdownInline tokens={t.tokens} /></a
      >
    {:else}
      <MarkdownInline tokens={t.tokens} />
    {/if}
  {:else if t.type === "image"}
    <!-- Rendered as a link, not an <img>: fetching it would reach out to
         whatever host the agent's output names. -->
    {@const href = safeHref(t.href)}
    {#if href}
      <a
        {href}
        title={t.title || href}
        target="_blank"
        rel="noreferrer noopener"
        onclick={(e) => open(e, href)}>🖼 {t.text || href}</a
      >
    {:else}{t.text ?? ""}{/if}
  {:else if t.type === "escape"}
    {t.text ?? ""}
  {:else}
    <!-- Raw HTML and anything marked adds later: shown as the source it is. -->
    {t.raw ?? t.text ?? ""}
  {/if}
{/each}
