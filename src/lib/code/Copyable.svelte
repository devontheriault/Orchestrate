<script lang="ts">
  import type { Snippet } from "svelte";

  /**
   * A block of code or output with a copy button in its top-right corner,
   * shown while the pointer is over the block. `text` is what gets copied —
   * the source as written, without line numbers or a `$` prompt. `flush`
   * is for a block with no padding of its own, as in a tool call's card: a
   * smaller button, right in the corner, so stacked blocks keep theirs apart.
   */
  let {
    text,
    flush = false,
    class: klass = "",
    children,
  }: { text: string; flush?: boolean; class?: string; children: Snippet } = $props();

  let copied = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;

  async function copy() {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // Older WebKitGTK has no async clipboard; the old way still works
      // inside a click.
      const area = document.createElement("textarea");
      area.value = text;
      area.style.position = "fixed";
      area.style.opacity = "0";
      document.body.append(area);
      area.select();
      const ok = document.execCommand("copy");
      area.remove();
      if (!ok) return;
    }
    copied = true;
    clearTimeout(timer);
    timer = setTimeout(() => (copied = false), 1500);
  }
</script>

<div class="copyable {klass}" class:flush>
  {@render children()}
  <button
    type="button"
    class="copy"
    class:copied
    title={copied ? "Copied" : "Copy"}
    aria-label={copied ? "Copied" : "Copy"}
    onclick={copy}
  >
    <svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true">
      {#if copied}
        <path
          d="M3 8.5l3.2 3L13 4.5"
          fill="none"
          stroke="currentColor"
          stroke-width="1.8"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
      {:else}
        <rect x="5.5" y="5.5" width="8" height="8" rx="1.6" fill="none" stroke="currentColor" stroke-width="1.4" />
        <path
          d="M10.5 3.2v-.2a1.5 1.5 0 0 0-1.5-1.5H4A1.5 1.5 0 0 0 2.5 3v5A1.5 1.5 0 0 0 4 9.5h.2"
          fill="none"
          stroke="currentColor"
          stroke-width="1.4"
          stroke-linecap="round"
        />
      {/if}
    </svg>
  </button>
</div>

<style>
  .copyable {
    position: relative;
  }

  .copy {
    position: absolute;
    top: 0.3rem;
    right: 0.3rem;
    display: grid;
    place-items: center;
    width: 1.6rem;
    height: 1.6rem;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface);
    color: var(--fg-muted);
    cursor: pointer;
    opacity: 0;
    transition:
      opacity var(--transition-fast),
      color var(--transition-fast),
      border-color var(--transition-fast);
  }

  .flush > .copy {
    top: 0;
    right: 0;
    width: 1.35rem;
    height: 1.35rem;
  }

  .copyable:hover > .copy,
  .copy:focus-visible,
  .copy.copied {
    opacity: 1;
  }

  /* A touch screen has no hover to reveal it, so it stays in view. */
  @media (hover: none) {
    .copy {
      opacity: 0.85;
    }
  }

  .copy:hover {
    color: var(--fg);
    border-color: var(--accent);
  }

  .copy:focus-visible {
    outline: none;
    border-color: var(--accent);
    box-shadow: var(--focus-ring);
  }

  .copy.copied {
    color: var(--success);
  }
</style>
