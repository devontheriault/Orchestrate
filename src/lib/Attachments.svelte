<script lang="ts">
  /**
   * The files attached to a prompt, as a row of chips — in the composer while
   * they're being gathered, and above the prompt in the transcript once sent.
   * A chip names the file; the full path is on hover, since two screenshots
   * from different folders can share a name. An image leads with a thumbnail
   * of itself, or with an icon if the webview can't draw it or it's gone.
   */
  import { SvelteSet } from "svelte/reactivity";
  import { api } from "./api";

  let {
    paths,
    onremove,
  }: {
    paths: string[];
    /** Offered in the composer only: a sent attachment can't be taken back. */
    onremove?: (path: string) => void;
  } = $props();

  const IMAGE = /\.(png|jpe?g|gif|webp|bmp|svg|avif|heic)$/i;

  /** Images that failed to load, so they fall back to the icon. */
  const broken = new SvelteSet<string>();

  /** Load an image's bytes into `img`, freeing them when the chip goes. */
  function thumbnail(path: string) {
    return (img: HTMLImageElement) => {
      let url: string | undefined;
      let gone = false;
      api
        .attachmentPreview(path)
        .then((bytes) => {
          if (gone) return;
          // SVG is the one format the webview won't sniff from the bytes.
          const type = /\.svg$/i.test(path) ? "image/svg+xml" : "";
          url = URL.createObjectURL(new Blob([bytes], { type }));
          img.src = url;
        })
        .catch(() => broken.add(path));
      return () => {
        gone = true;
        if (url) URL.revokeObjectURL(url);
      };
    };
  }

  function baseName(path: string): string {
    return path.split(/[\\/]/).pop() || path;
  }
</script>

{#if paths.length}
  <ul class="attachments">
    {#each paths as path (path)}
      <li class="chip" title={path}>
        {#if IMAGE.test(path) && !broken.has(path)}
          <img class="thumb" alt="" {@attach thumbnail(path)} onerror={() => broken.add(path)} />
        {:else}
          <svg class="icon" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
            {#if IMAGE.test(path)}
              <!-- A picture frame with a hill and a sun: reads as "image". -->
              <rect x="2" y="3" width="12" height="10" rx="1.6" fill="none" stroke="currentColor" stroke-width="1.4" />
              <circle cx="5.8" cy="6.6" r="1.1" fill="currentColor" />
              <path d="M2.8 12l3.7-3.6 2.4 2.3 1.6-1.5 2.7 2.8" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" />
            {:else}
              <!-- A page with a folded corner: reads as "file". -->
              <path d="M4 1.8h5.2L12.5 5v9.2H4z" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" />
              <path d="M9 2v3.2h3.3" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" />
            {/if}
          </svg>
        {/if}
        <span class="name">{baseName(path)}</span>
        {#if onremove}
          <button
            class="remove"
            onclick={() => onremove(path)}
            aria-label={`Remove ${baseName(path)}`}
            title="Remove"
          >
            <svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true">
              <path
                d="M3.5 3.5l9 9M12.5 3.5l-9 9"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
              />
            </svg>
          </button>
        {/if}
      </li>
    {/each}
  </ul>
{/if}

<style>
  .attachments {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    max-width: 16rem;
    padding: 0.2rem 0.55rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-pill);
    background: var(--code-bg);
    color: var(--fg);
    font-size: var(--text-xs);
    line-height: var(--leading-snug);
  }

  .icon {
    flex: none;
    color: var(--fg-muted);
  }

  /* Just big enough to tell one screenshot from another; the chip grows a
     little around it, and hugs it on the left as it does the ✕ on the right. */
  .thumb {
    flex: none;
    width: 1.5rem;
    height: 1.5rem;
    border-radius: 0.3rem;
    object-fit: cover;
    background: var(--border);
  }

  .chip:has(.thumb) {
    padding-left: 0.2rem;
  }

  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Tucked into the chip's right end, and a touch tighter there so the pill
     stays symmetric around the ✕. */
  .chip:has(.remove) {
    padding-right: 0.2rem;
  }

  .remove {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 1.25rem;
    height: 1.25rem;
    border: none;
    border-radius: var(--radius-pill);
    background: none;
    color: var(--fg-muted);
    cursor: pointer;
  }

  .remove:hover {
    background: var(--border);
    color: var(--danger);
  }

  .remove:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
</style>
