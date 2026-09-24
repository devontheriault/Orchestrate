<script lang="ts">
  import { store } from "$lib/state/store.svelte";
  import { hostNotice, HOST_NOTICE_DELAY } from "./hostNotice";

  const notice = $derived(hostNotice(store.host));
  const waits = $derived(store.host.state === "connecting");

  // Shown only once it has held for a moment, so a quick reconnect never
  // flashes a banner.
  let shown = $state(false);
  $effect(() => {
    if (!notice) {
      shown = false;
      return;
    }
    if (!waits) {
      shown = true;
      return;
    }
    const timer = setTimeout(() => (shown = true), HOST_NOTICE_DELAY);
    return () => clearTimeout(timer);
  });
</script>

{#if notice && shown}
  <div class="host-notice" role="status">{notice}</div>
{/if}

<style>
  .host-notice {
    background: var(--warning-soft-bg);
    border-bottom: 1px solid var(--warning-soft-border);
    color: var(--fg);
    padding: 0.55rem var(--pad-x);
    font-size: var(--text-lg);
    overflow-wrap: anywhere;
  }
</style>
