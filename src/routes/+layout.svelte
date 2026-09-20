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

    font-family:
      -apple-system, BlinkMacSystemFont, "Segoe UI", "Inter", Roboto, sans-serif;
    font-size: 14px;
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
