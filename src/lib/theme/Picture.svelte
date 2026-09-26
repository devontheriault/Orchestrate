<script lang="ts">
  import { theme } from "./theme.svelte";
</script>

<!-- The Picture theme's backdrop: the picture, frosted and veiled, behind the
     whole window. The theme's surfaces are see-through like Glass's, so this
     is what they show. Until a picture is picked it is a gradient, so the
     theme looks meant rather than broken. -->
{#if theme.resolved === "picture"}
  <div class="backdrop" aria-hidden="true">
    {#if theme.picture}
      {#key theme.picture}
        <div class="picture" style:background-image={`url("${theme.picture}")`}></div>
      {/key}
    {/if}
  </div>
{/if}

<style>
  /* Behind the page, above the document's own background. */
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: -1;
    overflow: hidden;
    pointer-events: none;
    background:
      radial-gradient(ellipse at 15% 20%, rgb(99 102 241 / 0.55), transparent 55%),
      radial-gradient(ellipse at 85% 75%, rgb(20 184 166 / 0.45), transparent 55%),
      radial-gradient(ellipse at 70% 10%, rgb(236 72 153 / 0.3), transparent 50%),
      rgb(7 9 18);
  }

  /* Oversized by twice the blur on each side, so the blur's soft edge falls
     outside the window instead of fading the picture in from its borders. */
  .picture {
    position: absolute;
    inset: calc(var(--picture-blur) * -2);
    background: center / cover no-repeat;
    filter: blur(var(--picture-blur));
    /* Its own layer, so the blur is worked out once rather than on every
       repaint of what scrolls over it. */
    will-change: transform;
    animation: fade-in var(--duration-slow) var(--ease);
  }

  /* The veil, over the picture and under the page. */
  .backdrop::after {
    content: "";
    position: absolute;
    inset: 0;
    background: rgb(7 9 18 / var(--picture-opacity));
  }

  @keyframes fade-in {
    from {
      opacity: 0;
    }
  }
</style>
