<script lang="ts">
  // JetBrains Mono, bundled so every platform gets the same face without a
  // system install. Upright and italic, variable weight.
  import "@fontsource-variable/jetbrains-mono";
  import "@fontsource-variable/jetbrains-mono/wght-italic.css";
  // Geist, for the wordmark alone.
  import "@fontsource-variable/geist";
  // The app's theme — colour, type, spacing, radii, motion, and the shared
  // button and input classes. Imported here because it is the one place every
  // page passes through. See src/lib/theme/theme.css.
  import "$lib/theme/theme.css";
  // The named palettes — Dracula, Nord, Tokyo Night and the rest. After
  // theme.css, because a palette overrides the colours it declares.
  import "$lib/theme/themes.css";
  import { theme } from "$lib/theme/theme.svelte";
  import WindowChrome from "$lib/layout/WindowChrome.svelte";
  import WindowResizeEdges from "$lib/layout/WindowResizeEdges.svelte";
  import Picture from "$lib/theme/Picture.svelte";

  let { children } = $props();

  // Keeps <html data-theme> in step with the preference, and with the OS while
  // the preference is "system". app.html set the first value before paint.
  theme.start();
</script>

<!-- The window's frame is the app's, not the OS's — see `build_main_window` in
     src-tauri/src/lib.rs. It belongs to the layout rather than the page for the
     same reason the OS bar used to sit outside the document: it's the window,
     not what the window is showing. -->
<Picture />
<WindowChrome />
<WindowResizeEdges />

{@render children()}
