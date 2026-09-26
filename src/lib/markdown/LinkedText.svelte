<script lang="ts">
  import Link from "./Link.svelte";
  import { linkify } from "./markdown";

  /** Plain text, shown as it is except that the URLs in it are links. */
  let { text }: { text: string } = $props();

  const parts = $derived(linkify(text));
</script>

{#each parts as p}{#if p.href}<Link href={p.href}>{p.text}</Link>{:else}{p.text}{/if}{/each}
