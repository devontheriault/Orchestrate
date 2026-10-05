<script lang="ts">
  /**
   * The open note: a header saying which file it is and whether it's saved, a
   * quiet column of Markdown to write in, and Read to see it rendered. With
   * no note open, a calm page that says how to start one.
   */
  import Markdown from "$lib/markdown/Markdown.svelte";
  import { dismissOnMove, menuStyle, placeMenu, stepActive, type Placement } from "$lib/menus/menu";
  import { DEFAULT_EFFORT, DEFAULT_MODE, DEFAULT_MODEL } from "$lib/picks";
  import { store } from "$lib/state/store.svelte";
  import MarkdownEditor from "./MarkdownEditor.svelte";
  import { folderOf, nameOf } from "./notes";
  import { notes } from "./notes.svelte";

  let {
    phone = false,
    mod = "Ctrl",
    reading = $bindable(false),
    oncreate,
  }: {
    phone?: boolean;
    mod?: string;
    /** Showing the note rendered rather than its Markdown. */
    reading?: boolean;
    oncreate: () => void;
  } = $props();

  let editor: MarkdownEditor | undefined = $state();

  export function focus() {
    if (reading) reading = false;
    queueMicrotask(() => editor?.focus());
  }

  const open = $derived(notes.open);
  const folder = $derived(open ? folderOf(open.path) : "");
  const words = $derived(open ? (open.text.match(/[\p{L}\p{N}][\p{L}\p{N}'’_-]*/gu)?.length ?? 0) : 0);

  const status = $derived.by(() => {
    switch (notes.saveState) {
      case "editing":
        return "Edited";
      case "saving":
        return "Saving…";
      case "failed":
        return "Not saved";
      default:
        return "Saved";
    }
  });

  // ---------------------------------------------------------------- rename

  let renaming = $state<string | null>(null);
  let renameEl: HTMLInputElement | undefined = $state();

  function startRename() {
    if (!open) return;
    renaming = open.path.replace(/\.(md|markdown)$/i, "");
    queueMicrotask(() => {
      renameEl?.focus();
      // The name, not the folders in front of it.
      const at = renaming!.lastIndexOf("/") + 1;
      renameEl?.setSelectionRange(at, renaming!.length);
    });
  }

  async function finishRename() {
    const to = renaming?.trim();
    renaming = null;
    if (!open || !to || to === open.path.replace(/\.(md|markdown)$/i, "")) return;
    await notes.rename(to);
  }

  // ----------------------------------------------------------- the menus

  type Row = { label: string; note?: string; danger?: boolean; run: () => void };
  let menu = $state<"more" | "agent" | null>(null);
  let placement = $state<Placement | null>(null);
  let active = $state(0);
  let moreEl: HTMLButtonElement | undefined = $state();
  let agentEl: HTMLButtonElement | undefined = $state();
  let menuEl: HTMLElement | undefined = $state();

  /** Hand the note to an Agent: Spawn one on a Project, with the note as its Task. */
  async function sendToAgent(projectId: string) {
    if (!open) return;
    await notes.flush();
    const text = open.text.trim();
    if (!text) return;
    store.selectProject(projectId);
    const p = store.prefs;
    const ok = await store.spawn(
      text,
      [],
      p.model ?? DEFAULT_MODEL,
      p.effort ?? DEFAULT_EFFORT,
      p.mode ?? DEFAULT_MODE,
    );
    sent = ok ? store.projects.find((x) => x.id === projectId)?.name ?? "the project" : null;
    if (!ok && store.error) notes.error = store.error;
    clearTimeout(sentTimer);
    sentTimer = setTimeout(() => (sent = null), 5000);
  }
  /** The Project an Agent was just Spawned on, said for a moment. */
  let sent = $state<string | null>(null);
  let sentTimer: ReturnType<typeof setTimeout> | undefined;

  const rows = $derived.by((): Row[] => {
    if (menu === "agent") {
      return store.projects.map((p) => ({
        label: p.name,
        run: () => {
          closeMenu();
          sendToAgent(p.id);
        },
      }));
    }
    return [
      { label: "Rename or move…", run: () => (closeMenu(), startRename()) },
      { label: reading ? "Edit" : "Read", note: `${mod}+E`, run: () => (closeMenu(), (reading = !reading)) },
      { label: "Move to Trash", danger: true, run: () => (closeMenu(), trash()) },
    ];
  });

  function openMenu(which: "more" | "agent") {
    const trigger = which === "more" ? moreEl : agentEl;
    if (!trigger) return;
    placement = placeMenu(trigger, { minWidth: 220, maxHeight: 360, align: "right" });
    active = 0;
    menu = which;
  }

  function closeMenu() {
    menu = null;
  }

  $effect(() => {
    if (!menu) return;
    return dismissOnMove(() => [menuEl, moreEl, agentEl], closeMenu);
  });

  $effect(() => {
    if (menu) menuEl?.focus();
  });

  function menuKey(e: KeyboardEvent) {
    const next = stepActive(e.key, active, rows.length);
    if (next !== null) {
      e.preventDefault();
      active = next;
    } else if (e.key === "Escape") {
      e.preventDefault();
      const back = menu === "more" ? moreEl : agentEl;
      closeMenu();
      back?.focus();
    } else if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      rows[active]?.run();
    } else if (e.key === "Tab") {
      closeMenu();
    }
  }

  // ----------------------------------------------------------------- trash

  /** Why the trash couldn't take the note, while the user decides. */
  let noTrash = $state<string | null>(null);

  async function trash() {
    noTrash = await notes.trash();
  }

  async function deleteForGood() {
    noTrash = null;
    await notes.trash(true);
  }

  $effect(() => {
    if (!noTrash) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && (noTrash = null);
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });
</script>

<section class="pane" class:phone>
  {#if open}
    <header>
      {#if phone}
        <button class="btn btn-ghost back" onclick={() => notes.close()} aria-label="Back to notes"
          >‹</button
        >
      {/if}
      {#if renaming !== null}
        <form
          class="rename"
          onsubmit={(e) => {
            e.preventDefault();
            finishRename();
          }}
        >
          <input
            bind:this={renameEl}
            class="input"
            bind:value={renaming}
            aria-label="Name, or folder/name to move it"
            title="A name, or folder/name to move it into a folder"
            onblur={finishRename}
            onkeydown={(e) => {
              if (e.key === "Escape") {
                e.preventDefault();
                renaming = null;
              }
            }}
          />
          <span class="ext">.md</span>
        </form>
      {:else}
        <button class="crumbs" onclick={startRename} title={`${open.path} — rename or move`}>
          {#if folder}<span class="crumb-folder">{folder}</span><span class="crumb-sep">/</span>{/if}
          <span class="crumb-name">{nameOf(open.path)}</span>
        </button>
      {/if}

      <div class="right">
        {#if sent}
          <span class="sent" role="status">Sent to an Agent in {sent}</span>
        {:else}
          <span class="status" class:failed={notes.saveState === "failed"} aria-live="polite">
            <span class="words">{words.toLocaleString()} {words === 1 ? "word" : "words"} ·</span>
            {status}
          </span>
        {/if}

        <div class="toggle" role="tablist" aria-label="Show">
          <button
            role="tab"
            aria-selected={!reading}
            class:on={!reading}
            onclick={() => (reading = false)}
            title={`Write (${mod}+E)`}>Write</button
          >
          <button
            role="tab"
            aria-selected={reading}
            class:on={reading}
            onclick={() => (reading = true)}
            title={`Read (${mod}+E)`}>Read</button
          >
        </div>

        {#if store.projects.length}
          <button
            bind:this={agentEl}
            class="btn btn-ghost btn-icon"
            aria-haspopup="menu"
            aria-expanded={menu === "agent"}
            aria-label="Send to an Agent"
            title="Send to an Agent: spawn one with this note as its Task"
            disabled={!open.text.trim() || store.spawning}
            onclick={() => (menu === "agent" ? closeMenu() : openMenu("agent"))}
          >
            <svg class="icon" viewBox="0 0 16 16" aria-hidden="true">
              <path d="M2.5 8h9M8 4l4 4-4 4" />
            </svg>
          </button>
        {/if}

        <button
          bind:this={moreEl}
          class="btn btn-ghost btn-icon"
          aria-haspopup="menu"
          aria-expanded={menu === "more"}
          aria-label="More"
          title="More"
          onclick={() => (menu === "more" ? closeMenu() : openMenu("more"))}
        >
          <svg class="icon dots" viewBox="0 0 16 16" aria-hidden="true">
            <circle cx="3.5" cy="8" r="1.1" />
            <circle cx="8" cy="8" r="1.1" />
            <circle cx="12.5" cy="8" r="1.1" />
          </svg>
        </button>
      </div>
    </header>

    {#if open.conflict}
      <div class="conflict" role="alert">
        <p>
          {#if open.conflict.current}
            <strong>This note changed on disk</strong> while you were editing it — in another
            editor, another window, or by an Agent.
          {:else}
            <strong>This note was deleted or moved on disk</strong> while you were editing it.
          {/if}
          Nothing has been overwritten.
        </p>
        <div class="choices">
          {#if open.conflict.current}
            <button class="btn btn-sm" onclick={() => notes.resolve("theirs")}>Use the disk's version</button>
            <button class="btn btn-sm" onclick={() => notes.resolve("both")}>Keep both</button>
            <button class="btn btn-sm btn-warning" onclick={() => notes.resolve("mine")}>Keep mine</button>
          {:else}
            <button class="btn btn-sm" onclick={() => notes.resolve("theirs")}>Close it</button>
            <button class="btn btn-sm btn-warning" onclick={() => notes.resolve("mine")}>Save it again</button>
          {/if}
        </div>
      </div>
    {/if}

    <div class="page">
      <div class="column">
        {#if reading}
          <div class="reading">
            {#if open.text.trim()}
              <Markdown text={open.text} />
            {:else}
              <p class="nothing">Nothing written yet.</p>
            {/if}
          </div>
        {:else}
          {#key open.path}
            <MarkdownEditor
              bind:this={editor}
              text={open.text}
              onedit={(t) => notes.edit(t)}
              jump={open.line}
              onjumped={() => notes.open && (notes.open.line = null)}
              placeholder={"# A title\n\nStart writing…"}
            />
          {/key}
        {/if}
      </div>
    </div>
  {:else}
    <div class="blank-page">
      <svg class="mark" viewBox="0 0 48 48" aria-hidden="true">
        <path d="M14 8h15l7 7v25a2 2 0 0 1-2 2H14a2 2 0 0 1-2-2V10a2 2 0 0 1 2-2z" />
        <path d="M29 8v7h7M18 24h12M18 30h12M18 36h7" />
      </svg>
      <p>Pick a note, or start a new one.</p>
      <button class="btn btn-lg" onclick={oncreate}>New note</button>
      <p class="keys keys-only">
        <kbd>{mod}+N</kbd> new note · <kbd>{mod}+K</kbd> search · <kbd>{mod}+E</kbd> read
      </p>
    </div>
  {/if}
</section>

{#if menu && placement}
  <div
    bind:this={menuEl}
    class="popover menu"
    role="menu"
    tabindex="-1"
    style={menuStyle(placement)}
    onkeydown={menuKey}
  >
    {#if menu === "agent"}
      <div class="menu-head">Spawn an Agent with this note as its Task, in</div>
    {/if}
    {#each rows as row, i}
      <div
        class="menu-item"
        class:danger={row.danger}
        role="menuitem"
        tabindex="-1"
        data-active={i === active}
        onpointermove={() => (active = i)}
        onclick={row.run}
        onkeydown={() => {}}
      >
        <span class="menu-label">{row.label}</span>
        {#if row.note}<span class="menu-note keys-only">{row.note}</span>{/if}
      </div>
    {/each}
  </div>
{/if}

{#if noTrash}
  <div
    class="backdrop"
    role="presentation"
    onclick={(e) => e.target === e.currentTarget && (noTrash = null)}
  >
    <div class="dialog" role="alertdialog" aria-modal="true" aria-labelledby="no-trash-title">
      <h2 id="no-trash-title">Delete this note for good?</h2>
      <p>
        It can't go to the trash here ({noTrash}), so deleting it can't be undone.
      </p>
      <footer>
        <button class="btn" onclick={() => (noTrash = null)}>Cancel</button>
        <button class="btn btn-danger" onclick={deleteForGood}>Delete for good</button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .pane {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    background: var(--surface);
    container-type: inline-size;
  }

  /* A narrow pane keeps the name and the controls, and lets the rest go. */
  @container (max-width: 40rem) {
    .crumb-folder,
    .crumb-sep,
    .words {
      display: none;
    }
  }

  header {
    flex: none;
    display: flex;
    align-items: center;
    gap: var(--space-4);
    min-height: 2.8rem;
    padding: var(--pad-y) var(--pad-x);
  }

  .back {
    font-size: var(--text-3xl);
    padding: 0 var(--space-4);
    margin-left: calc(-1 * var(--space-3));
  }

  .crumbs {
    min-width: 0;
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    margin-left: calc(-1 * var(--space-3));
    background: transparent;
    border: var(--border-width) solid transparent;
    border-radius: var(--radius-md);
    color: var(--fg);
    font-size: var(--text-md);
    cursor: text;
    white-space: nowrap;
    overflow: hidden;
  }

  .crumbs:hover {
    border-color: var(--border);
  }

  .crumb-folder,
  .crumb-sep {
    color: var(--fg-muted);
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .crumb-name {
    font-weight: var(--weight-medium);
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .rename {
    flex: 1;
    min-width: 0;
    max-width: 28rem;
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .ext {
    color: var(--fg-muted);
    font-size: var(--text-sm);
  }

  .right {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex: none;
  }

  .status,
  .sent {
    font-size: var(--text-xs);
    color: var(--fg-muted);
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }

  .status.failed {
    color: var(--danger-text);
  }

  .sent {
    color: var(--success);
  }

  .toggle {
    display: flex;
    padding: 2px;
    border-radius: var(--radius-md);
    background: var(--hover);
  }

  .toggle button {
    padding: 0.15rem 0.6rem;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--fg-muted);
    font-size: var(--text-xs);
    cursor: pointer;
  }

  .toggle button.on {
    background: var(--surface);
    color: var(--fg);
    box-shadow: 0 1px 2px rgb(0 0 0 / 0.12);
  }

  .icon {
    width: 0.95rem;
    height: 0.95rem;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.5;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .icon.dots {
    fill: currentColor;
    stroke: none;
  }

  .conflict {
    flex: none;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-4) var(--space-6);
    margin: 0 var(--pad-x) var(--space-4);
    padding: var(--space-4) var(--space-5);
    border: var(--border-width) solid var(--warning-soft-border);
    border-radius: var(--radius-lg);
    background: var(--warning-soft-bg);
    font-size: var(--text-sm);
  }

  .conflict p {
    flex: 1 1 18rem;
    margin: 0;
  }

  .choices {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }

  /* The note scrolls as one page; the column keeps a reading measure. */
  .page {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: var(--space-7) var(--pad-x) 30vh;
    cursor: text;
  }

  .column {
    max-width: 44rem;
    min-height: 100%;
    margin: 0 auto;
  }

  .pane.phone .page {
    padding-top: var(--space-5);
    padding-bottom: calc(40vh + var(--safe-bottom));
  }

  .pane.phone .column {
    --note-size: var(--text-xl);
  }

  /* Read mode: the same column and size as writing, so switching doesn't
     make the page jump about. */
  .reading {
    font-size: var(--text-2xl);
    line-height: 1.7;
  }

  .reading :global(.md h1) {
    font-size: 1.5em;
    margin-top: 0.4em;
  }

  .reading :global(.md p),
  .reading :global(.md ul),
  .reading :global(.md ol) {
    margin-bottom: 0.9em;
  }

  .nothing {
    color: var(--fg-muted);
    font-style: italic;
  }

  .blank-page {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-4);
    padding: var(--space-8);
    color: var(--fg-muted);
    text-align: center;
  }

  .blank-page p {
    margin: 0;
    font-size: var(--text-lg);
  }

  .mark {
    width: 3.2rem;
    height: 3.2rem;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.4;
    stroke-linecap: round;
    stroke-linejoin: round;
    opacity: 0.45;
    margin-bottom: var(--space-3);
  }

  .keys {
    margin-top: var(--space-5) !important;
    font-size: var(--text-xs) !important;
  }

  kbd {
    font-size: var(--text-2xs);
    border: var(--border-width) solid var(--border);
    border-radius: var(--radius-xs);
    padding: 0.05rem 0.3rem;
  }

  .menu-head {
    padding: var(--space-3) var(--menu-item-pad-x);
    font-size: var(--text-2xs);
    color: var(--fg-muted);
  }

  .menu-item.danger {
    color: var(--danger-text);
  }

  .backdrop {
    position: fixed;
    inset: 0;
    z-index: var(--z-modal);
    background: var(--scrim);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: var(--space-6);
  }

  .dialog {
    width: min(26rem, 100%);
    padding: var(--space-6);
    background: var(--float-bg, var(--surface));
    -webkit-backdrop-filter: var(--float-filter, none);
    backdrop-filter: var(--float-filter, none);
    border: var(--border-width) solid var(--border);
    border-radius: var(--radius-xl);
    box-shadow: var(--shadow-modal);
  }

  .dialog h2 {
    margin: 0 0 var(--space-4);
    font-size: var(--text-2xl);
  }

  .dialog p {
    margin: 0;
    font-size: var(--text-md);
    color: var(--fg-muted);
  }

  .dialog footer {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-3);
    margin-top: var(--space-6);
  }
</style>
