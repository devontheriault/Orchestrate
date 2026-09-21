<script lang="ts">
  import { theme } from "./theme.svelte";

  /** Rail mode: icon only, matching the Usage button it sits above. */
  let { collapsed = false }: { collapsed?: boolean } = $props();

  // One button rather than three: the choice is small, it is made rarely, and
  // a segmented control would cost the rail more width than it has. The icon
  // shows what you are looking at; the label says where the value came from.
  const next = $derived(
    theme.pref === "system" ? "Light" : theme.pref === "light" ? "Dark" : "System",
  );
</script>

<button
  class="btn btn-ghost toggle"
  class:collapsed
  onclick={() => theme.cycle()}
  title={`Theme: ${theme.label} — click for ${next}`}
  aria-label={`Theme: ${theme.label}. Switch to ${next}`}
>
  <span class="icon" aria-hidden="true">
    {#if theme.pref === "system"}
      <!-- Half-filled: the app is not the one deciding. -->
      <svg viewBox="0 0 16 16" width="13" height="13">
        <circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" stroke-width="1.4" />
        <path d="M8 2a6 6 0 0 1 0 12z" fill="currentColor" />
      </svg>
    {:else if theme.resolved === "dark"}
      <svg viewBox="0 0 16 16" width="13" height="13">
        <path
          d="M13.5 9.6A5.6 5.6 0 0 1 6.4 2.5a5.8 5.8 0 1 0 7.1 7.1z"
          fill="none"
          stroke="currentColor"
          stroke-width="1.4"
          stroke-linejoin="round"
        />
      </svg>
    {:else}
      <svg viewBox="0 0 16 16" width="13" height="13">
        <circle cx="8" cy="8" r="3.1" fill="none" stroke="currentColor" stroke-width="1.4" />
        <path
          d="M8 1v1.6M8 13.4V15M1 8h1.6M13.4 8H15M3.1 3.1l1.1 1.1M11.8 11.8l1.1 1.1M12.9 3.1l-1.1 1.1M4.2 11.8l-1.1 1.1"
          stroke="currentColor"
          stroke-width="1.4"
          stroke-linecap="round"
        />
      </svg>
    {/if}
  </span>
  {#if !collapsed}<span class="label">{theme.label}</span>{/if}
</button>

<style>
  /* Sized to sit under the Usage button as its equal, not as a `.btn` in a
     row of buttons: full width, quiet, and square when the rail is narrow. */
  .toggle {
    width: 100%;
    justify-content: flex-start;
    gap: var(--space-3);
    padding: 0.35rem 0.45rem;
    font-size: var(--text-sm);
    font-weight: var(--weight-normal);
  }

  .icon {
    display: flex;
    align-items: center;
    line-height: var(--leading-none);
  }

  .label {
    flex: 1;
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .toggle.collapsed {
    justify-content: center;
    padding: 0.35rem 0;
  }
</style>
