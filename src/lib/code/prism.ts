/**
 * Prism and the grammars we ship, as one chunk that `highlight.svelte.ts`
 * loads the first time code is on screen — so the app's own startup never
 * pays for it.
 *
 * Every grammar registers itself on the global `Prism` the core creates, and
 * some are built from others (tsx from jsx and typescript, svelte from markup
 * and javascript), so these imports run in dependency order. Supporting
 * another language is one more line here.
 */
import "prismjs/components/prism-core";
import "prismjs/components/prism-markup";
import "prismjs/components/prism-css";
import "prismjs/components/prism-clike";
import "prismjs/components/prism-javascript";
import "prismjs/components/prism-typescript";
import "prismjs/components/prism-jsx";
import "prismjs/components/prism-tsx";
import "prism-svelte";
import "prismjs/components/prism-json";
import "prismjs/components/prism-bash";
import "prismjs/components/prism-rust";
import "prismjs/components/prism-python";
import "prismjs/components/prism-go";
import "prismjs/components/prism-yaml";
import "prismjs/components/prism-toml";
import "prismjs/components/prism-markdown";
import "prismjs/components/prism-diff";
import "prismjs/components/prism-sql";

export default (globalThis as unknown as { Prism: typeof import("prismjs") }).Prism;
