<script lang="ts">
  let { children } = $props();
</script>

{@render children()}

<style>
  :global(:root) {
    --fg: #111;
    --fg-muted: #6b7280;
    --surface: #ffffff;
    --panel-bg: #f7f7f8;
    --border: #e5e7eb;
    --hover: #eef0f3;
    --selected: #e6efff;
    --accent: #3b82f6;
    --code-bg: #eef0f3;
    --running: #16a34a;
    --running-bg: rgba(34, 197, 94, 0.14);
    --attention: #d97706;
    --attention-bg: rgba(245, 158, 11, 0.16);

    /* Panes are sized from the window, not fixed: they give up width as the
       window narrows and take it back as it widens, within readable bounds. */
    --pane-projects: clamp(12rem, 20vw, 20rem);
    --rail: 3.4rem;
    /* Breathing room scales too, so a narrow window spends it on content. */
    --pad-x: clamp(0.6rem, 1.1vw, 1.1rem);
    --pad-y: clamp(0.5rem, 0.8vh, 0.8rem);
    /* Prose stops growing past a comfortable measure on a wide window. */
    --measure: 96ch;

    font-family:
      -apple-system, BlinkMacSystemFont, "Segoe UI", "Inter", Roboto, sans-serif;
    /* Everything below is in rem, so the whole UI scales with the window. */
    font-size: clamp(13px, 0.5vw + 8.6px, 16px);
    color: var(--fg);
    background: var(--surface);
  }

  :global(html, body) {
    margin: 0;
    padding: 0;
    height: 100%;
    overflow: hidden;
  }

  :global(body) {
    display: flex;
    flex-direction: column;
    height: 100vh;
    height: 100dvh;
  }

  @media (prefers-color-scheme: dark) {
    :global(:root) {
      --fg: #e5e7eb;
      --fg-muted: #9ca3af;
      --surface: #16181d;
      --panel-bg: #1c1e24;
      --border: #2a2d34;
      --hover: #23262e;
      --selected: #253044;
      --accent: #60a5fa;
      --code-bg: #23262e;
      --running: #4ade80;
      --running-bg: rgba(34, 197, 94, 0.18);
      --attention: #fbbf24;
      --attention-bg: rgba(245, 158, 11, 0.2);
    }
  }

  :global(button) {
    font-family: inherit;
  }

  :global(*) {
    box-sizing: border-box;
  }
</style>
