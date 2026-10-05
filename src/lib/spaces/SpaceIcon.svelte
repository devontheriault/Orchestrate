<script lang="ts">
  /** A Space's icon, sized by the font size it sits in and drawn in its colour. */
  let { icon }: { icon: string[] } = $props();
</script>

<svg viewBox="0 0 20 20" aria-hidden="true">
  {#each icon as d (d)}<path {d} />{/each}
</svg>

<style>
  svg {
    width: 1em;
    height: 1em;
    flex: none;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.5;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
</style>
