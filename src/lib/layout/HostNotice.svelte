<script lang="ts">
  import { hosts } from "$lib/state/hosts.svelte";
  import { hostNotice, phoneNotice, HOST_NOTICE_DELAY } from "./hostNotice";

  const own = hosts.own;
  const notice = $derived(
    own ? hostNotice(hosts.status(own) ?? { state: "connecting", error: null }) : phoneNotice(hosts.list),
  );
  const waits = $derived(!own || (hosts.status(own)?.state ?? "connecting") === "connecting");

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
