import { defineConfig, type Plugin } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";
import process from "node:process";

const host = process.env.TAURI_DEV_HOST;

/**
 * vite-plugin-svelte serves a component's `<style>` as a virtual module, and
 * fills it from what compiling the component left behind. Nothing makes the
 * compile happen first: when the stylesheet is asked for before its component,
 * as a webview revalidating its cache on a fresh dev server can, the plugin
 * logs "failed to load virtual css module", Vite falls back to reading the
 * file, and the component's whole source is served — and cached — as its CSS.
 * So compile the component before the stylesheet loads.
 */
function compileSvelteBeforeItsCss(): Plugin {
  return {
    name: "compile-svelte-before-its-css",
    apply: "serve",
    load: {
      order: "pre",
      filter: { id: /[?&]svelte&type=style&lang\.css$/ },
      async handler(id) {
        const env = this.environment;
        if (env.mode !== "dev") return;
        const file = id.slice(0, id.indexOf("?"));
        if (this.getModuleInfo(file)?.meta.svelte?.css) return;
        const { root } = env.config;
        const url = file.startsWith(`${root}/`) ? file.slice(root.length) : `/@fs${file}`;
        await env.transformRequest(url);
      },
    },
  };
}

export default defineConfig(() => ({
  plugins: [compileSvelteBeforeItsCss(), sveltekit()],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || "127.0.0.1",
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
    // 4. keep the webview from caching modules: WebKit fetches a cached page's
    //    old modules again when the next session starts, so a file deleted
    //    since shows up as a 404 in the dev server's log
    headers: { "Cache-Control": "no-store" },
  },
}));
