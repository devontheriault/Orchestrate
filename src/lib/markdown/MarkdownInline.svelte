<script lang="ts">
  import MarkdownInline from "./MarkdownInline.svelte";
  import Link from "./Link.svelte";
  import LinkedText from "./LinkedText.svelte";
  import { decodeEntities, safeHref, type Token } from "./markdown";

  let { tokens }: { tokens?: Token[] } = $props();
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
    <!-- `http://localhost:5173` is how a dev server gets named. -->
    <code><LinkedText text={t.text ?? ""} /></code>
  {:else if t.type === "br"}
    <br />
  {:else if t.type === "link"}
    {@const href = safeHref(t.href)}
    {#if href}
      <Link {href} title={t.title}><MarkdownInline tokens={t.tokens} /></Link>
    {:else}
      <MarkdownInline tokens={t.tokens} />
    {/if}
  {:else if t.type === "image"}
    <!-- Rendered as a link, not an <img>: fetching it would reach out to
         whatever host the agent's output names. -->
    {@const href = safeHref(t.href)}
    {#if href}
      <Link {href} title={t.title}>🖼 {t.text || href}</Link>
    {:else}{t.text ?? ""}{/if}
  {:else if t.type === "escape"}
    {t.text ?? ""}
  {:else}
    <!-- Raw HTML and anything marked adds later: shown as the source it is. -->
    {t.raw ?? t.text ?? ""}
  {/if}
{/each}
