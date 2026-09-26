// Tauri serves the build from disk, with no server to render pages on request.
// So the one page is rendered at build time instead (in development, by the
// dev server on each load): the window then paints the app's frame from the
// HTML itself, as soon as it arrives, rather than staying see-through until
// every module has loaded and run. The app then takes over that markup
// (hydration) instead of drawing its own.
//
// Rendering ahead of time means no window: anything that runs on import, or
// while a component is set up, can't touch `window`, `document`,
// `localStorage` or the Tauri API. Do that in an `$effect`, `onMount`, or an
// event handler. And what the page first draws must be what a window with no
// Host data yet would draw — the pre-paint script in `src/app.html` carries
// the few stored settings the layout depends on.
//
// See: https://svelte.dev/docs/kit/page-options#prerender
// See: https://v2.tauri.app/start/frontend/sveltekit/
export const prerender = true;
