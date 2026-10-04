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
  import NoteEditor from "./NoteEditor.svelte";
  import NoteSidebar from "./NoteSidebar.svelte";
  import { notes } from "./notes.svelte";

  let sidebar: NoteSidebar | undefined = $state();
  let editor: NoteEditor | undefined = $state();
  let reading = $state(false);
  let mod = $state("Ctrl");

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
    {#if !viewport.phone || !notes.open}
      <NoteSidebar bind:this={sidebar} phone={viewport.phone} {mod} oncreate={create} />
    {/if}
    {#if !viewport.phone || notes.open}
      <NoteEditor bind:this={editor} bind:reading phone={viewport.phone} {mod} oncreate={create} />
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
