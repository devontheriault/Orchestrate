<script lang="ts">
  /**
   * The Notes Space (ADR 0017): the notes on one Host, which are Markdown
   * files in a folder. Fills whatever box it's given — the list on the left
   * and the open note on the right, or on a phone one at a time.
   *
   * Keys: Ctrl/⌘+N a new note, Ctrl/⌘+K search, Ctrl/⌘+E read or write,
   * Ctrl/⌘+S save now (it saves itself anyway).
   */
  import { onDestroy, onMount } from "svelte";
  import { hosts } from "$lib/state/hosts.svelte";
  import { viewport } from "$lib/layout/viewport.svelte";
  import { panes, MIN_DETAIL, MIN_SIDE } from "$lib/layout/panes.svelte";
  import PaneDivider from "$lib/layout/PaneDivider.svelte";
  import PhoneStack from "$lib/layout/PhoneStack.svelte";
  import { space } from "$lib/spaces/space.svelte";
  import NoteEditor from "./NoteEditor.svelte";
  import NoteSidebar from "./NoteSidebar.svelte";
  import { notes } from "./notes.svelte";

  let sidebar: NoteSidebar | undefined = $state();
  let editor: NoteEditor | undefined = $state();
  let reading = $state(false);
  let mod = $state("Ctrl");
  /** The sidebar's size, unrounded, so the title bar's lead segment meets its edge exactly. */
  let sideBox = $state<readonly ResizeObserverSize[]>();

  // The title bar tops the list; a phone has no list beside the note.
  $effect(() => {
    panes.lead.notes = viewport.phone ? 0 : (sideBox?.[0]?.inlineSize ?? 0);
  });

  onMount(() => {
    if (document.documentElement.dataset.frame === "mac") mod = "⌘";
    notes.start();
  });

  onDestroy(() => {
    notes.stop();
  });

  // A Host that comes back — or answers for the first time — is asked again.
  $effect(() => {
    if (hosts.reachable(notes.host) && notes.problem) notes.refresh();
  });

  async function create() {
    reading = false;
    if (await notes.create()) editor?.focus();
  }

  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      // The rail keeps Notes mounted behind the other Spaces; its keys are
      // only its own while it's showing.
      if (space.current !== "notes") return;
      if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
      const key = e.key.toLowerCase();
      if (key === "n" && !e.shiftKey) {
        e.preventDefault();
        create();
      } else if (key === "k" || (key === "f" && e.shiftKey)) {
        e.preventDefault();
        sidebar?.focusSearch();
      } else if (key === "e" && !e.shiftKey && notes.open) {
        e.preventDefault();
        reading = !reading;
        if (!reading) editor?.focus();
      } else if (key === "s" && !e.shiftKey) {
        e.preventDefault();
        notes.flush();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  // A note opened elsewhere — a search hit, a list row — opens in Write.
  $effect(() => {
    notes.open?.path;
    reading = false;
  });
</script>

<div class="notes-space" class:phone={viewport.phone}>
  {#if notes.error}
    <div class="error" role="alert">
      <span>{notes.error}</span>
      <button onclick={() => (notes.error = null)} aria-label="Dismiss">×</button>
    </div>
  {/if}
  <div class="panes">
    {#if !viewport.phone}
      <div
        class="side"
        style:flex-basis={panes.basis("notes")}
        style:min-width="{MIN_SIDE.notes}px"
        style:max-width="calc(100% - {MIN_DETAIL}px)"
        bind:borderBoxSize={sideBox}
      >
        <NoteSidebar bind:this={sidebar} {mod} oncreate={create} />
      </div>
      <PaneDivider
        label="Resize notes list"
        min={MIN_SIDE.notes}
        minLast={MIN_DETAIL}
        onresize={(w) => panes.setSide("notes", w)}
        onreset={() => panes.setSide("notes", null)}
      />
      <NoteEditor bind:this={editor} bind:reading {mod} oncreate={create} />
    {:else}
      <PhoneStack space="notes" open={!!notes.open} onclose={() => notes.close()}>
        {#snippet list()}<NoteSidebar bind:this={sidebar} phone {mod} oncreate={create} />{/snippet}
        {#snippet detail()}
          <NoteEditor bind:this={editor} bind:reading phone {mod} oncreate={create} />
        {/snippet}
      </PhoneStack>
    {/if}
  </div>
</div>

<style>
  .notes-space {
    flex: 1;
    min-width: 0;
    min-height: 0;
    height: 100%;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .panes {
    flex: 1;
    min-height: 0;
    display: flex;
  }

  .side {
    flex: 0 1 clamp(13rem, 20vw, 20rem);
    display: flex;
    min-height: 0;
  }

  .error {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    padding: 0.45rem var(--pad-x);
    background: var(--danger-soft-bg);
    border-bottom: var(--border-width) solid var(--danger-soft-border);
    color: var(--danger-text);
    font-size: var(--text-sm);
    overflow-wrap: anywhere;
  }

  .error button {
    flex: none;
    background: transparent;
    border: none;
    color: inherit;
    font-size: var(--text-2xl);
    line-height: var(--leading-none);
    cursor: pointer;
  }
</style>
