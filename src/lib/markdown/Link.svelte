<script lang="ts">
  import type { Snippet } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";

  /** `href` must already have passed `safeHref` or come out of `linkify`. */
  let { href, title, children }: { href: string; title?: string; children: Snippet } = $props();

  /**
   * The transcript lives in the app's only window, so letting a link navigate
   * would replace the app with the page. Links go to the OS browser instead.
   */
  function open(e: MouseEvent) {
    e.preventDefault();
    openUrl(href).catch((err) => console.error("couldn't open link", href, err));
  }
</script>

<!-- One line: links sit inside `<pre>`, where any whitespace here would show. -->
<a {href} title={title || href} target="_blank" rel="noreferrer noopener" onclick={open}>{@render children()}</a>

<style>
  a {
    color: var(--accent);
    text-decoration: underline;
    text-underline-offset: 2px;
    overflow-wrap: anywhere;
    cursor: pointer;
  }
</style>
