<script lang="ts">
  import { formatShell } from "./shell";

  /** A shell command, laid out one step per line and coloured by part. */
  let { command }: { command: string } = $props();

  const tokens = $derived(formatShell(command));
</script>

<!-- The prompt sits in its own column, so a step broken onto a line of its
     own lines up under the first rather than under the `$`. -->
<div class="shell">
  <span class="prompt" aria-hidden="true">$</span>
  <pre>{#each tokens as t, i (i)}{#if t.kind === "text"}{t.text}{:else}<span class={t.kind}
          >{t.text}</span
        >{/if}{/each}</pre>
</div>

<style>
  .shell {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    column-gap: 0.6em;
    font-family: var(--font-mono);
    font-size: var(--text-sm);
  }

  .prompt {
    color: var(--fg-muted);
    user-select: none;
  }

  pre {
    background: transparent;
    margin: 0;
    padding: 0;
    font-size: inherit;
    color: var(--fg);
    white-space: pre-wrap;
    /* Break long paths anywhere, but only once a line can't wrap at a space. */
    overflow-wrap: break-word;
    max-height: min(60vh, 34rem);
    overflow: auto;
  }

  .cmd { font-weight: var(--weight-semibold); }
  .flag { color: var(--completed); }
  .string { color: var(--diff-add-fg); }
  .var { color: var(--delivered); }
  .op, .comment { color: var(--fg-muted); }
  .comment { font-style: italic; }
</style>
