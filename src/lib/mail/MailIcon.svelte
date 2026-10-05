<script lang="ts" module>
  /** Every glyph the Mail Space draws, as SVG paths on a 24-unit grid. */
  const PATHS: Record<string, string> = {
    inbox: "M22 12h-6l-2 3h-4l-2-3H2 M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z",
    important: "M3 7.5 12 3l9 4.5v9L12 21l-9-4.5z M12 8v5 M12 16h.01",
    flagged:
      "m12 2 3.09 6.26L22 9.27l-5 4.87 1.18 6.88L12 17.77l-6.18 3.25L7 14.14 2 9.27l6.91-1.01z",
    drafts: "M12 20h9 M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4z",
    sent: "m22 2-7 20-4-9-9-4z M22 2 11 13",
    archive: "M3 4h18v4H3z M5 8v11a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1V8 M10 12h4",
    all: "M4 6h16v12H4z M4 7l8 6 8-6 M2 4v12 M22 4v12",
    junk: "M7.86 2h8.28L22 7.86v8.28L16.14 22H7.86L2 16.14V7.86z M12 8v4 M12 16h.01",
    trash: "M3 6h18 M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2 M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6 M10 11v6 M14 11v6",
    folder: "M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.93a2 2 0 0 1-1.66-.9l-.82-1.2A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13c0 1.1.9 2 2 2z",
    search: "M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14z M21 21l-4.35-4.35",
    clip: "m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48",
    refresh: "M21 12a9 9 0 1 1-2.64-6.36L21 8 M21 3v5h-5",
    back: "m15 18-6-6 6-6",
    more: "M12 5h.01 M12 12h.01 M12 19h.01",
    agent: "M12 3v3 M8 9h8a3 3 0 0 1 3 3v5a3 3 0 0 1-3 3H8a3 3 0 0 1-3-3v-5a3 3 0 0 1 3-3z M9.5 14h.01 M14.5 14h.01 M2 14h3 M19 14h3",
    image: "M3 3h18v18H3z M8.5 10a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3z M21 15l-5-5L5 21",
    download: "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4 M7 10l5 5 5-5 M12 15V3",
    unread: "M4 4h16v16H4z M4 8l8 5 8-5",
    shield: "M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z",
    mail: "M4 4h16a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z M22 6l-10 7L2 6",
    close: "M18 6 6 18 M6 6l12 12",
  };
</script>

<script lang="ts">
  let { name, size = "1em" }: { name: string; size?: string } = $props();
</script>

<svg
  class="icon"
  viewBox="0 0 24 24"
  width={size}
  height={size}
  aria-hidden="true"
  fill={name === "flagged" ? "currentColor" : "none"}
>
  <path d={PATHS[name] ?? PATHS.folder} />
</svg>

<style>
  .icon {
    flex: none;
    stroke: currentColor;
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
</style>
