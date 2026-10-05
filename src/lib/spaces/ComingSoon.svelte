<script lang="ts">
  /**
   * What a Space shows until it's built: its icon, its name, and what it will
   * be. Each Space's real component replaces this in its entry in `spaces.ts`.
   */
  import type { Space } from "./spaces";
  import SpaceIcon from "./SpaceIcon.svelte";

  let { space }: { space: Space } = $props();

  /** What each Space will hold, in ADR 0017's terms. */
  const PROMISES: Record<string, string> = {
    notes:
      "Your notes as Markdown files in a folder you choose. Your editor, git and your Agents can all read them too.",
    calendar:
      "Your calendar, straight from your provider over CalDAV, so this and your phone always agree.",
    mail: "Your mail over IMAP, searchable here and by your Agents. Only you ever send.",
  };
</script>

<div class="soon">
  <div class="card">
    <div class="badge-icon"><SpaceIcon icon={space.icon} /></div>
    <span class="eyebrow">Coming soon</span>
    <h1>{space.label}</h1>
    {#if PROMISES[space.id]}<p>{PROMISES[space.id]}</p>{/if}
  </div>
</div>

<style>
  .soon {
    flex: 1;
    min-width: 0;
    display: grid;
    place-items: center;
    padding: var(--space-8) var(--pad-x);
    background: var(--surface);
    overflow: auto;
  }

  .card {
    max-width: 26rem;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-3);
    text-align: center;
  }

  /* The rail's icon, large, on a tint of the accent: the one place the app
     shows a Space's mark at more than glyph size. */
  .badge-icon {
    width: 4.5rem;
    height: 4.5rem;
    margin-bottom: var(--space-4);
    display: grid;
    place-items: center;
    border-radius: var(--radius-xl);
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    box-shadow: inset 0 0 0 var(--border-width) color-mix(in srgb, var(--accent) 22%, transparent);
    font-size: 2.25rem;
  }

  .eyebrow {
    font-size: var(--text-2xs);
    font-weight: var(--weight-semibold);
    color: var(--fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }

  h1 {
    margin: 0;
    font-size: var(--text-3xl);
    font-weight: var(--weight-semibold);
    color: var(--fg);
  }

  p {
    margin: var(--space-2) 0 0;
    font-size: var(--text-md);
    line-height: var(--leading-relaxed);
    color: var(--fg-muted);
  }
</style>
