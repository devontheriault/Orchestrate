<script lang="ts">
  /**
   * Which folder the notes are kept in on their Host, for the user to change.
   * The setting lives on the Host itself. Which Host is the picker beside this.
   */
  import { open as pickFolder } from "@tauri-apps/plugin-dialog";
  import { api } from "$lib/api";
  import { mobile } from "$lib/layout/platform";
  import { dismissOnMove, menuStyle, placeMenu, stepActive, type Placement } from "$lib/menus/menu";
  import { hosts } from "$lib/state/hosts.svelte";
  import { notes } from "./notes.svelte";

  let open = $state(false);
  let active = $state(0);
  let placement = $state<Placement | null>(null);
  let triggerEl: HTMLButtonElement | undefined = $state();
  let menuEl: HTMLElement | undefined = $state();

  /** `~/Notes` on the Host, once asked. */
  let defaultFolder = $state<string | null>(null);
  /** Typing a folder by hand: for a Host on another machine, which this one can't browse. */
  let typing = $state<string | null>(null);

  type Row = { label: string; note?: string; run: () => void };

  const rows = $derived.by((): Row[] => {
    const out: Row[] = [{ label: "Change folder…", run: change }];
    if (defaultFolder && defaultFolder !== notes.folder) {
      out.push({ label: "Use ~/Notes", note: "default", run: () => use(null) });
    }
    return out;
  });

  async function show() {
    if (!triggerEl) return;
    placement = placeMenu(triggerEl, { minWidth: 260, maxHeight: 420, align: "right" });
    active = 0;
    typing = null;
    open = true;
    try {
      defaultFolder = (await api.notesFolder(notes.host)).default;
    } catch {
      defaultFolder = null;
    }
  }

  function close() {
    open = false;
    typing = null;
  }

  async function change() {
    // The folder picker browses this machine, so it only helps for its own Host.
    if (!mobile && notes.host === hosts.own) {
      close();
      const picked = await pickFolder({ directory: true, multiple: false, defaultPath: notes.folder });
      if (typeof picked === "string") await notes.setFolder(picked);
      return;
    }
    typing = notes.folder;
  }

  async function use(folder: string | null) {
    close();
    await notes.setFolder(folder);
  }

  $effect(() => {
    if (!open) return;
    return dismissOnMove(() => [menuEl, triggerEl], close);
  });

  $effect(() => {
    if (open && typing === null) menuEl?.focus();
  });

  function keydown(e: KeyboardEvent) {
    if (typing !== null) return;
    const next = stepActive(e.key, active, rows.length);
    if (next !== null) {
      e.preventDefault();
      active = next;
    } else if (e.key === "Escape") {
      e.preventDefault();
      close();
      triggerEl?.focus();
    } else if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      rows[active]?.run();
    } else if (e.key === "Tab") {
      close();
    }
  }
</script>

<button
  bind:this={triggerEl}
  class="btn btn-ghost btn-icon trigger"
  aria-haspopup="menu"
  aria-expanded={open}
  aria-label="Where notes are kept"
  title="Where notes are kept"
  onclick={() => (open ? close() : show())}
>
  <svg viewBox="0 0 16 16" aria-hidden="true">
    <circle cx="3.5" cy="8" r="1.1" />
    <circle cx="8" cy="8" r="1.1" />
    <circle cx="12.5" cy="8" r="1.1" />
  </svg>
</button>

{#if open && placement}
  <div
    bind:this={menuEl}
    class="popover menu notes-menu"
    role="menu"
    tabindex="-1"
    style={menuStyle(placement)}
    onkeydown={keydown}
  >
    <div class="where">
      <span class="field-label">Notes folder</span>
      <code title={notes.folder}>{notes.folder || "…"}</code>
      {#if hosts.list.length > 1}
        <span class="field-hint">on {hosts.label(notes.host)}</span>
      {/if}
    </div>
    {#if typing !== null}
      <form
        class="typing"
        onsubmit={(e) => {
          e.preventDefault();
          if (typing?.trim()) use(typing.trim());
        }}
      >
        <!-- svelte-ignore a11y_autofocus -->
        <input
          class="input input-mono"
          bind:value={typing}
          autofocus
          aria-label="Notes folder"
          placeholder="/home/you/Notes"
          onkeydown={(e) => e.key === "Escape" && (e.preventDefault(), close())}
        />
        <button class="btn btn-primary btn-sm" type="submit">Use</button>
      </form>
    {/if}
    <div class="menu-sep"></div>
    {#each rows as row, i}
      <div
        class="menu-item"
        role="menuitem"
        tabindex="-1"
        data-active={i === active}
        onpointermove={() => (active = i)}
        onclick={row.run}
        onkeydown={() => {}}
      >
        <span class="menu-label">{row.label}</span>
        {#if row.note}<span class="menu-note">{row.note}</span>{/if}
      </div>
    {/each}
  </div>
{/if}

<style>
  .trigger {
    width: 1.6rem;
    height: 1.6rem;
  }

  .trigger svg {
    width: 0.95rem;
    height: 0.95rem;
    fill: currentColor;
  }

  .where {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3) var(--menu-item-pad-x) var(--space-2);
  }

  .where code {
    font-size: var(--text-xs);
    color: var(--fg);
    overflow-wrap: anywhere;
  }

  .typing {
    display: flex;
    gap: var(--space-3);
    padding: var(--space-2) var(--menu-item-pad-x);
  }
</style>
