<script lang="ts">
  import { dragRegion } from "./platform";

  /** The mark alone, for when there is no room for the name beside it. */
  let { markOnly = false }: { markOnly?: boolean } = $props();
</script>

<!-- The mark is the "O" of the name, so the two are one lockup rather than an
     icon beside a label: the text starts at "r". Geometry is the brand kit's
     64-unit grid, with the bars in the text colour and only the centre bar in
     the signal green, so it reads on every theme. -->
<span
  class="logo"
  class:mark-only={markOnly}
  role="img"
  aria-label="Orchestrate"
  data-tauri-drag-region={dragRegion}
>
  <svg class="mark" viewBox="0 0 64 64" aria-hidden="true">
    <rect x="5.8" y="22.35" width="4.4" height="19.3" rx="2.2" />
    <rect x="11.8" y="13.49" width="4.4" height="37.03" rx="2.2" />
    <rect x="17.8" y="7.81" width="4.4" height="48.37" rx="2.2" />
    <rect x="23.8" y="7.25" width="4.4" height="13.35" rx="2.2" />
    <rect x="23.8" y="43.39" width="4.4" height="13.35" rx="2.2" />
    <rect class="signal" x="29.8" y="5" width="4.4" height="14" rx="2.2" />
    <rect class="signal" x="29.8" y="45" width="4.4" height="14" rx="2.2" />
    <rect x="35.8" y="7.25" width="4.4" height="13.35" rx="2.2" />
    <rect x="35.8" y="43.39" width="4.4" height="13.35" rx="2.2" />
    <rect x="41.8" y="7.81" width="4.4" height="48.37" rx="2.2" />
    <rect x="47.8" y="13.49" width="4.4" height="37.03" rx="2.2" />
    <rect x="53.8" y="22.35" width="4.4" height="19.3" rx="2.2" />
  </svg>
  {#if !markOnly}<span class="word" aria-hidden="true" data-tauri-drag-region={dragRegion}>rchestrate</span>{/if}
</span>

<style>
  /* Sized by font-size, so the mark and the name scale as one. */
  .logo {
    display: inline-flex;
    align-items: baseline;
    min-width: 0;
    font-family: var(--font-brand);
    font-weight: var(--weight-semibold);
    letter-spacing: -0.045em;
    line-height: var(--leading-none);
    color: var(--fg);
    white-space: nowrap;
  }

  /* 64 grid units to the kit's 69px type. The box's bottom edge sits on the
     baseline, so it drops by the 5 empty units under the bars to set the "O"
     on the same line as the letters after it. */
  .mark {
    flex: none;
    width: 0.93em;
    height: 0.93em;
    transform: translateY(calc(0.93em * 5 / 64));
    fill: currentColor;
    /* Clicks fall through to the span, which is a window drag handle. */
    pointer-events: none;
  }

  /* With no text to share a baseline with, it centres like any icon. */
  .mark-only .mark {
    transform: none;
  }

  .mark .signal {
    fill: var(--brand-signal);
  }

  .word {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
