<script lang="ts">
  import { tick } from "svelte";
  import { latest, participants, when } from "./list";
  import { mail } from "./mail.svelte";
  import MailIcon from "./MailIcon.svelte";

  let {
    compact = false,
    onfolders,
    onopen,
    search = $bindable(),
  }: {
    /** The folder column is hidden, so the header offers the folders. */
    compact?: boolean;
    onfolders?: () => void;
    /** Called after a conversation is opened, for a layout showing one column. */
    onopen?: () => void;
    /** The search field, so `/` can reach it. */
    search?: HTMLInputElement;
  } = $props();

  let typed = $state(mail.query);
  let listEl: HTMLUListElement | undefined = $state();

  const heading = $derived(
    mail.results
      ? `Results for “${mail.query}”`
      : (mail.folders.find((f) => f.path === mail.folder)?.name ?? mail.folder),
  );

  function open(id: string) {
    void mail.select(id);
    onopen?.();
  }

  function searchKey(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      void mail.search(typed);
      search?.blur();
    } else if (e.key === "Escape") {
      e.preventDefault();
      typed = "";
      mail.clearSearch();
      search?.blur();
    }
  }

  // Keep the selected row in view as j and k walk the list.
  $effect(() => {
    const id = mail.selectedId;
    if (!id || !listEl) return;
    void tick().then(() => {
      listEl
        ?.querySelector<HTMLElement>(`[data-thread="${CSS.escape(id)}"]`)
        ?.scrollIntoView({ block: "nearest" });
    });
  });
</script>

<section class="threads" aria-label="Conversations">
  <header>
    <div class="search">
      <MailIcon name="search" />
      <input
        bind:this={search}
        bind:value={typed}
        onkeydown={searchKey}
        type="search"
        placeholder="Search all mail"
        aria-label="Search all mail"
        spellcheck="false"
      />
      {#if mail.searching}
        <span class="spinner" aria-label="Searching"></span>
      {:else}
        <kbd class="keys-only">/</kbd>
      {/if}
    </div>
  </header>

  <div class="bar">
    {#if compact && !mail.results}
      <button class="folder-pick" onclick={onfolders} aria-label="Show folders">
        <MailIcon name="folder" />
        <span>{heading}</span>
        <span class="chev" aria-hidden="true">›</span>
      </button>
    {:else}
      <h2>{heading}</h2>
    {/if}
    {#if mail.results}
      <button
        class="btn btn-ghost btn-sm"
        onclick={() => {
          typed = "";
          mail.clearSearch();
        }}
      >
        Clear
      </button>
    {:else}
      <button
        class="btn btn-ghost btn-icon refresh"
        class:spinning={mail.loading}
        onclick={() => mail.refresh()}
        aria-label="Check for new mail"
        title="Check for new mail (r)"
      >
        <MailIcon name="refresh" />
      </button>
    {/if}
  </div>

  {#if mail.list.length === 0}
    <div class="empty">
      {#if mail.loading || mail.searching}
        Loading…
      {:else if mail.results}
        Nothing found. The server searched every folder for it.
      {:else}
        Nothing here.
      {/if}
    </div>
  {:else}
    <ul bind:this={listEl} role="listbox" aria-label={heading}>
      {#each mail.list as t (t.id)}
        {@const last = latest(t)}
        <li
          role="option"
          aria-selected={t.id === mail.selectedId}
          data-thread={t.id}
          class:selected={t.id === mail.selectedId}
          class:unread={t.unread > 0}
        >
          <button class="row" onclick={() => open(t.id)}>
            <div class="line">
              <span class="dot" aria-hidden="true"></span>
              <span class="who">{participants(t, mail.me)}</span>
              {#if t.messages.length > 1}
                <span class="n">{t.messages.length}</span>
              {/if}
              <span class="flags">
                {#if t.messages.some((m) => m.attachments)}
                  <span title="Has attachments"><MailIcon name="clip" /></span>
                {/if}
                {#if t.messages.some((m) => m.flagged)}
                  <span class="star" title="Flagged"><MailIcon name="flagged" /></span>
                {/if}
              </span>
              <time>{when(t.date)}</time>
            </div>
            <div class="subject">{t.subject}</div>
            <div class="snippet">{last.snippet}</div>
            {#if last.labels.length}
              <div class="labels">
                {#each last.labels.slice(0, 3) as l}
                  <span class="label">{l}</span>
                {/each}
              </div>
            {/if}
          </button>
        </li>
      {/each}
    </ul>
  {/if}

  <footer class="keys-only" aria-hidden="true">
    <span><kbd>j</kbd><kbd>k</kbd> move</span>
    <span><kbd>e</kbd> archive</span>
    <span><kbd>#</kbd> delete</span>
    <span><kbd>u</kbd> unread</span>
    <span><kbd>a</kbd> to agent</span>
  </footer>
</section>

<style>
  .threads {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    height: 100%;
    background: var(--surface);
  }

  header {
    padding: var(--pad-y) var(--pad-x) var(--space-3);
  }

  .search {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: 0 var(--space-4);
    height: 2.1rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--panel-bg);
    color: var(--fg-muted);
    transition: border-color var(--transition-fast), box-shadow var(--transition-fast);
  }

  .search:focus-within {
    border-color: var(--accent);
    box-shadow: var(--focus-ring);
    background: var(--surface);
  }

  .search input {
    flex: 1;
    min-width: 0;
    border: none;
    outline: none;
    background: transparent;
    color: var(--fg);
    font: inherit;
    font-size: var(--text-md);
  }

  .search input::-webkit-search-cancel-button {
    display: none;
  }

  kbd {
    font-family: var(--font-mono);
    font-size: var(--text-3xs);
    color: var(--fg-muted);
    border: 1px solid var(--border);
    border-bottom-width: 2px;
    border-radius: var(--radius-xs);
    padding: 0 0.3rem;
    line-height: 1.4;
  }

  .spinner {
    width: 0.8rem;
    height: 0.8rem;
    border: 2px solid var(--border);
    border-top-color: var(--accent);
    border-radius: var(--radius-circle);
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .bar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: 0 var(--pad-x) var(--space-3);
    min-height: 2rem;
    border-bottom: 1px solid var(--border);
  }

  h2,
  .folder-pick {
    flex: 1;
    min-width: 0;
    margin: 0;
    font-size: var(--text-xl);
    font-weight: var(--weight-semibold);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .folder-pick {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    border: none;
    background: transparent;
    color: var(--fg);
    font-family: inherit;
    padding: var(--space-2) 0;
    cursor: pointer;
    text-align: left;
  }

  .folder-pick :global(.icon) {
    color: var(--fg-muted);
  }

  .chev {
    color: var(--fg-muted);
  }

  .refresh :global(.icon) {
    width: 0.95rem;
    height: 0.95rem;
  }

  .refresh.spinning :global(.icon) {
    animation: spin 0.9s linear infinite;
  }

  .empty {
    padding: var(--space-8) var(--pad-x);
    text-align: center;
    color: var(--fg-muted);
    font-size: var(--text-md);
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    overflow-y: auto;
    flex: 1;
    min-height: 0;
  }

  li {
    border-bottom: 1px solid color-mix(in srgb, var(--border) 60%, transparent);
  }

  .row {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 0.12rem;
    padding: 0.6rem var(--pad-x) 0.65rem calc(var(--pad-x) + 0.2rem);
    border: none;
    background: transparent;
    color: var(--fg);
    font: inherit;
    text-align: left;
    cursor: pointer;
    position: relative;
  }

  .row:hover {
    background: var(--hover);
  }

  li.selected .row {
    background: var(--selected);
  }

  .line {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
  }

  .dot {
    position: absolute;
    left: calc(var(--pad-x) * 0.35);
    top: 1.05rem;
    width: 0.45rem;
    height: 0.45rem;
    border-radius: var(--radius-circle);
    background: transparent;
  }

  li.unread .dot {
    background: var(--accent);
  }

  .who {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--text-md);
    color: var(--fg-muted);
  }

  li.unread .who {
    color: var(--fg);
    font-weight: var(--weight-semibold);
  }

  .n {
    flex: none;
    font-size: var(--text-2xs);
    color: var(--fg-muted);
    border: 1px solid var(--border);
    border-radius: var(--radius-pill);
    padding: 0 0.35rem;
    line-height: 1.35;
  }

  .flags {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-left: auto;
    color: var(--fg-muted);
  }

  .flags :global(.icon) {
    width: 0.8rem;
    height: 0.8rem;
  }

  .star {
    color: var(--warning);
  }

  time {
    flex: none;
    font-size: var(--text-xs);
    color: var(--fg-muted);
    font-variant-numeric: tabular-nums;
  }

  li.unread time {
    color: var(--accent);
    font-weight: var(--weight-medium);
  }

  .subject {
    font-size: var(--text-md);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  li.unread .subject {
    font-weight: var(--weight-semibold);
  }

  .snippet {
    font-size: var(--text-sm);
    color: var(--fg-muted);
    line-height: var(--leading-snug);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .labels {
    display: flex;
    gap: var(--space-2);
    margin-top: var(--space-1);
  }

  .label {
    font-size: var(--text-3xs);
    color: var(--fg-muted);
    background: var(--code-bg);
    border-radius: var(--radius-xs);
    padding: 0.05rem 0.35rem;
  }

  footer {
    flex: none;
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-5);
    padding: var(--space-3) var(--pad-x);
    border-top: 1px solid var(--border);
    font-size: var(--text-2xs);
    color: var(--fg-muted);
  }

  footer kbd {
    margin-right: 0.15rem;
  }
</style>
