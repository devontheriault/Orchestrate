<script lang="ts">
  /**
   * The Notes Space's left pane: where the notes are, search, the folders,
   * and the notes, newest first. The list draws only the rows on screen, so a
   * folder of thousands of notes scrolls as lightly as one of ten.
   */
  import { hosts } from "$lib/state/hosts.svelte";
  import { notes } from "./notes.svelte";
  import { folderOf, folderRows, mark, onScreen, when } from "./notes";
  import NotesPlace from "./NotesPlace.svelte";

  let {
    phone = false,
    mod = "Ctrl",
    oncreate,
  }: {
    phone?: boolean;
    /** How the modifier key is written on this platform: ⌘ or Ctrl. */
    mod?: string;
    oncreate: () => void;
  } = $props();

  let searchEl: HTMLInputElement | undefined = $state();
  let listEl: HTMLElement | undefined = $state();

  export function focusSearch() {
    searchEl?.focus();
    searchEl?.select();
  }

  /** Folders folded shut in the tree, by path. */
  let folded = $state<Record<string, boolean>>({});
  const tree = $derived(folderRows(notes.folders, notes.notes, folded));

  const searching = $derived(notes.hits !== null);
  /** What the list shows: search hits while searching, else the notes. */
  const rows = $derived(
    notes.hits ??
      notes.shown.map((n) => ({ ...n, excerpt: n.snippet, line: 0 })),
  );

  /** The row the keyboard is on in the search results. */
  let active = $state(0);
  $effect(() => {
    notes.hits;
    active = 0;
  });

  // Rows are one fixed height, so which are on screen is arithmetic.
  let scrollTop = $state(0);
  let viewHeight = $state(600);
  let rowPx = $state(0);
  const ROW_REM = 4.9;
  $effect(() => {
    const measure = () => {
      rowPx = ROW_REM * parseFloat(getComputedStyle(document.documentElement).fontSize);
    };
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  });
  const slice = $derived(onScreen(scrollTop, viewHeight, rowPx, rows.length));

  // Back to the top whenever what the list holds changes kind.
  $effect(() => {
    notes.within;
    searching;
    if (listEl) listEl.scrollTop = 0;
  });

  /** Keep the keyboard's row in view. */
  function reveal(i: number) {
    if (!listEl || !rowPx) return;
    const top = i * rowPx;
    if (top < listEl.scrollTop) listEl.scrollTop = top;
    else if (top + rowPx > listEl.scrollTop + listEl.clientHeight)
      listEl.scrollTop = top + rowPx - listEl.clientHeight;
  }

  function searchKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      if (notes.query) notes.clearSearch();
      else searchEl?.blur();
      return;
    }
    if (!rows.length) return;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      active = Math.max(0, Math.min(rows.length - 1, active + (e.key === "ArrowDown" ? 1 : -1)));
      reveal(active);
    } else if (e.key === "Enter") {
      e.preventDefault();
      const row = rows[active];
      if (row) notes.openNote(row.path, row.line || null);
    }
  }

  /** ↑ and ↓ on the list step through the notes, opening each. */
  function listKey(e: KeyboardEvent) {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const at = rows.findIndex((r) => r.path === notes.open?.path);
    const next = Math.max(0, Math.min(rows.length - 1, at + (e.key === "ArrowDown" ? 1 : -1)));
    const row = rows[next];
    if (!row) return;
    reveal(next);
    notes.openNote(row.path, row.line || null);
  }

  const total = $derived(notes.notes.length);
</script>

<aside class:phone>
  <header>
    <span class="title">Notes</span>
    <div class="actions">
      <button
        class="btn btn-icon add"
        onclick={oncreate}
        title={`New note (${mod}+N)`}
        aria-label="New note">+</button
      >
    </div>
  </header>

  <NotesPlace {phone} />

  <div class="search">
    <svg class="glass" viewBox="0 0 16 16" aria-hidden="true">
      <circle cx="7" cy="7" r="4.5" />
      <path d="M10.5 10.5 14 14" />
    </svg>
    <input
      bind:this={searchEl}
      class="input"
      type="search"
      placeholder="Search notes"
      aria-label="Search notes"
      value={notes.query}
      oninput={(e) => notes.search(e.currentTarget.value)}
      onkeydown={searchKey}
    />
    {#if !notes.query}
      <kbd class="hint keys-only">{mod}+K</kbd>
    {/if}
  </div>

  {#if tree.length && !searching}
    <nav class="folders" aria-label="Folders">
      <button
        class="folder"
        class:on={notes.within === ""}
        onclick={() => notes.narrow("")}
        aria-current={notes.within === "" ? "true" : undefined}
      >
        <span class="fold-space"></span>
        <span class="folder-name">All notes</span>
        <span class="count">{total}</span>
      </button>
      {#each tree as f (f.path)}
        <div class="folder-row" style:--depth={f.depth + 1}>
          {#if f.parent}
            <button
              class="fold"
              class:open={!folded[f.path]}
              onclick={() => (folded[f.path] = !folded[f.path])}
              aria-label={folded[f.path] ? `Show folders in ${f.name}` : `Hide folders in ${f.name}`}
              aria-expanded={!folded[f.path]}>›</button
            >
          {/if}
          <button
            class="folder"
            class:on={notes.within === f.path}
            onclick={() => notes.narrow(f.path)}
            aria-current={notes.within === f.path ? "true" : undefined}
            title={f.path}
          >
            <span class="fold-space"></span>
            <svg class="folder-icon" viewBox="0 0 16 16" aria-hidden="true">
              <path d="M2 4.5c0-.6.4-1 1-1h3l1.4 1.5H13c.6 0 1 .4 1 1V12c0 .6-.4 1-1 1H3c-.6 0-1-.4-1-1z" />
            </svg>
            <span class="folder-name">{f.name}</span>
            <span class="count">{f.count}</span>
          </button>
        </div>
      {/each}
    </nav>
  {/if}

  {#if !notes.loaded}
    <!-- The Host hasn't answered yet. -->
    <div class="list"></div>
  {:else if notes.problem}
    <div class="empty" role="status">
      {#if !hosts.reachable(notes.host)}
        Your notes live on {hosts.label(notes.host)}, which can't be reached right now. Pick
        another machine above to keep notes there instead.
      {:else}
        Couldn't read your notes: {notes.problem}
      {/if}
    </div>
  {:else if searching && rows.length === 0}
    <div class="empty" role="status">Nothing matches “{notes.query}”.</div>
  {:else if !searching && total === 0}
    <div class="empty">
      <p>No notes yet.</p>
      <p class="where">They're Markdown files in <code>{notes.folder}</code>.</p>
      <button class="btn btn-lg" onclick={oncreate}>New note</button>
    </div>
  {:else if !searching && rows.length === 0}
    <div class="empty">
      <p>No notes in this folder yet.</p>
      <button class="btn btn-lg" onclick={oncreate}>New note here</button>
    </div>
  {:else}
    <div
      class="list"
      bind:this={listEl}
      bind:clientHeight={viewHeight}
      onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}
      onkeydown={listKey}
      role="listbox"
      tabindex="-1"
      aria-label={searching ? "Search results" : "Notes"}
    >
      <div class="spacer" style:height={`${rows.length * rowPx}px`}>
        {#each rows.slice(slice.start, slice.end) as row, i (row.path)}
          {@const index = slice.start + i}
          {@const dir = folderOf(row.path)}
          <button
            class="note"
            class:open={notes.open?.path === row.path}
            class:active={searching && index === active}
            style:transform={`translateY(${index * rowPx}px)`}
            style:height={`${rowPx}px`}
            role="option"
            aria-selected={notes.open?.path === row.path}
            onclick={() => notes.openNote(row.path, row.line || null)}
          >
            <span class="note-top">
              <span class="note-title">
                {#each mark(row.title, searching ? notes.query : "") as part}
                  {#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}
                {/each}
              </span>
              <span class="note-when">{when(row.modified)}</span>
            </span>
            <span class="note-snippet">
              {#if row.excerpt}
                {#each mark(row.excerpt, searching ? notes.query : "") as part}
                  {#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}
                {/each}
              {:else}
                <span class="blank">No text yet</span>
              {/if}
            </span>
            {#if dir && dir !== notes.within}
              <span class="note-folder">{dir}</span>
            {/if}
          </button>
        {/each}
      </div>
    </div>
  {/if}
</aside>

<style>
  /* Fills the box NotesSpace sizes, whose divider is the seam to the right. */
  aside {
    flex: 1;
    min-width: 0;
    background: var(--panel-bg);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  aside.phone {
    padding-bottom: var(--safe-bottom);
  }

  header {
    padding: var(--pad-y) var(--pad-x);
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    min-height: 2.8rem;
  }

  .title {
    font-size: var(--text-sm);
    font-weight: var(--weight-semibold);
    color: var(--fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }

  .add {
    width: 1.6rem;
    height: 1.6rem;
    background: transparent;
    font-size: var(--text-2xl);
    line-height: var(--leading-none);
  }

  aside.phone .add {
    width: 2.75rem;
    height: 2.75rem;
  }

  .search {
    position: relative;
    margin: 0 var(--pad-x) var(--space-4);
  }

  .search .input {
    padding-left: 1.9rem;
    padding-right: 3.6rem;
    background: var(--surface);
    border-radius: var(--radius-lg);
  }

  /* The platform's clear button doesn't follow the theme; Escape clears. */
  .search .input::-webkit-search-cancel-button {
    display: none;
  }

  .glass {
    position: absolute;
    left: 0.6rem;
    top: 50%;
    width: 0.85rem;
    height: 0.85rem;
    transform: translateY(-50%);
    fill: none;
    stroke: var(--fg-muted);
    stroke-width: 1.5;
    stroke-linecap: round;
    pointer-events: none;
  }

  .hint {
    position: absolute;
    right: 0.5rem;
    top: 50%;
    transform: translateY(-50%);
    font-size: var(--text-3xs);
    color: var(--fg-muted);
    border: var(--border-width) solid var(--border);
    border-radius: var(--radius-xs);
    padding: 0.05rem 0.3rem;
    pointer-events: none;
  }

  .folders {
    flex: none;
    max-height: 32%;
    overflow-y: auto;
    padding: 0 var(--space-3) var(--space-3);
    border-bottom: var(--border-width) solid var(--border);
    margin-bottom: var(--space-2);
  }

  .folder-row {
    position: relative;
  }

  .folder {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: 0.3rem var(--space-3) 0.3rem calc(var(--space-3) + (var(--depth, 0) - 1) * 0.9rem);
    background: transparent;
    border: none;
    border-radius: var(--radius-md);
    color: var(--fg);
    font-size: var(--text-md);
    text-align: left;
    cursor: pointer;
  }

  .folder:hover {
    background: var(--hover);
  }

  .folder.on {
    background: var(--selected);
    color: var(--accent);
  }

  .fold-space {
    flex: none;
    width: 0.7rem;
  }

  .fold {
    position: absolute;
    z-index: 1;
    left: calc(var(--space-3) + (var(--depth, 0) - 1) * 0.9rem - 0.15rem);
    top: 50%;
    width: 1rem;
    height: 1rem;
    transform: translateY(-50%);
    padding: 0;
    background: transparent;
    border: none;
    color: var(--fg-muted);
    font-size: var(--text-lg);
    line-height: var(--leading-none);
    cursor: pointer;
    transition: transform var(--transition-fast);
  }

  .fold.open {
    transform: translateY(-50%) rotate(90deg);
  }

  .folder-icon {
    flex: none;
    width: 0.9rem;
    height: 0.9rem;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.3;
    stroke-linejoin: round;
    opacity: 0.75;
  }

  .folder-name {
    flex: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .count {
    flex: none;
    font-size: var(--text-2xs);
    color: var(--fg-muted);
    font-variant-numeric: tabular-nums;
  }

  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
    outline: none;
  }

  .spacer {
    position: relative;
  }

  .note {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 0.2rem;
    padding: 0 var(--pad-x);
    background: transparent;
    border: none;
    border-bottom: var(--border-width) solid color-mix(in srgb, var(--border) 60%, transparent);
    color: var(--fg);
    text-align: left;
    cursor: pointer;
    overflow: hidden;
  }

  .note:hover {
    background: var(--hover);
  }

  .note.active {
    background: var(--hover);
    box-shadow: inset 2px 0 0 var(--fg-muted);
  }

  .note.open {
    background: var(--selected);
    box-shadow: inset 2px 0 0 var(--accent);
  }

  .note-top {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
    min-width: 0;
  }

  .note-title {
    flex: 1;
    min-width: 0;
    font-size: var(--text-lg);
    font-weight: var(--weight-semibold);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .note-when {
    flex: none;
    font-size: var(--text-2xs);
    color: var(--fg-muted);
    font-variant-numeric: tabular-nums;
  }

  .note-snippet {
    font-size: var(--text-xs);
    line-height: var(--leading-snug);
    color: var(--fg-muted);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    overflow-wrap: anywhere;
  }

  .blank {
    font-style: italic;
    opacity: 0.7;
  }

  .note-folder {
    font-size: var(--text-3xs);
    color: var(--fg-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    opacity: 0.8;
  }

  /* With a folder line, the snippet keeps to one so the row holds its height. */
  .note:has(.note-folder) .note-snippet {
    -webkit-line-clamp: 1;
    line-clamp: 1;
  }

  mark {
    background: color-mix(in srgb, var(--warning) 28%, transparent);
    color: inherit;
    border-radius: 2px;
  }

  .empty {
    padding: 1.5rem 1rem;
    text-align: center;
    color: var(--fg-muted);
    font-size: var(--text-md);
    overflow-wrap: anywhere;
  }

  .empty p {
    margin: 0 0 var(--space-3);
  }

  .where code {
    font-size: var(--text-xs);
  }

  .empty :global(button) {
    margin-top: var(--space-4);
  }

  aside.phone .note {
    padding-inline: 1rem;
  }
</style>
