// The app is one page, rendered at build time into `build/index.html` (see
// src/routes/+layout.ts), so adapter-static needs no fallback page — one would
// be written over the rendered one.
// See: https://v2.tauri.app/start/frontend/sveltekit/ for more info
import adapter from "@sveltejs/adapter-static";

/** @type {import('@sveltejs/kit').Config} */
const config = {
  kit: {
    adapter: adapter(),
  },
};

export default config;
