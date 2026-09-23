<script lang="ts">
  import type { Span } from "./highlight.svelte";

  /**
   * Highlighted code, as coloured runs of text with nothing around them —
   * the caller decides the element they sit in and how it wraps. Every view
   * of code shares these colours from here.
   */
  let { spans }: { spans: Span[] } = $props();
</script>

{#each spans as s}{#if s.kind}<span class={s.kind}>{s.text}</span
    >{:else}{s.text}{/if}{/each}

<style>
  /* The same palette as `ShellCommand`, so a command and the code it runs
     read as one language. A span carries every token type it's nested in;
     where two apply, the later rule here wins, so the narrower kinds of
     token come last. */
  .tok-inserted { color: var(--diff-add-fg); }
  .tok-deleted { color: var(--diff-del-fg); }
  .tok-coord { color: var(--fg-muted); }

  .tok-tag,
  .tok-selector,
  .tok-atrule { color: var(--delivered); }

  /* Script embedded in markup — a Svelte `{expression}` inside a tag — is
     plain code again, not part of the tag's colour; its own tokens below
     still colour it. */
  .tok-language-javascript,
  .tok-language-css { color: var(--fg); }

  .tok-keyword,
  .tok-important { color: var(--delivered); }

  .tok-function,
  .tok-class-name,
  .tok-builtin,
  .tok-macro,
  .tok-property,
  .tok-attr-name { color: var(--completed); }

  .tok-string,
  .tok-char,
  .tok-attr-value,
  .tok-regex,
  .tok-url { color: var(--diff-add-fg); }

  .tok-number,
  .tok-boolean,
  .tok-constant,
  .tok-symbol { color: var(--warning-text); }

  .tok-punctuation,
  .tok-operator { color: var(--fg-muted); }

  .tok-comment,
  .tok-prolog,
  .tok-doctype,
  .tok-cdata { color: var(--fg-muted); font-style: italic; }

  .tok-bold { font-weight: var(--weight-semibold); }
  .tok-italic { font-style: italic; }
</style>
