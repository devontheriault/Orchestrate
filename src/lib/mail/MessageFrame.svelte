<script lang="ts">
  import { FRAME_SANDBOX, frameDocument } from "./frame";

  /**
   * A message's HTML, sanitized by the Host, in a frame that can't run
   * scripts or reach the app (see `frame.ts`). It fills the box it is given
   * and scrolls inside it: a frame without scripts can't say how tall it is.
   */
  let { html, images }: { html: string; images: boolean } = $props();

  const doc = $derived(frameDocument(html, images));
</script>

<iframe
  title="Message"
  sandbox={FRAME_SANDBOX}
  referrerpolicy="no-referrer"
  srcdoc={doc}
></iframe>

<style>
  iframe {
    display: block;
    width: 100%;
    height: 100%;
    border: none;
    border-radius: var(--radius-lg);
    background: #fff;
    box-shadow: 0 0 0 1px var(--border);
  }
</style>
