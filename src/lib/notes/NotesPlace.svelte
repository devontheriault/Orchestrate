<script lang="ts">
  /**
   * Where the notes are: which Host keeps them and the folder there, always on
   * show at the top of the list. Clicking it changes either — the Host for any
   * other the window knows (or one added there and then), the folder for any
   * on that Host. The folder setting lives on the Host itself.
   */
  import { open as pickFolder } from "@tauri-apps/plugin-dialog";
  import { api } from "$lib/api";
  import { mobile } from "$lib/layout/platform";
  import { dismissOnMove, menuStyle, placeMenu, stepActive, type Placement } from "$lib/menus/menu";
  import HostsDialog from "$lib/sidebar/HostsDialog.svelte";
  import { hosts } from "$lib/state/hosts.svelte";
  import { notes } from "./notes.svelte";
  import { fromHome, notesHostChoices } from "./notes";

  let { phone = false }: { phone?: boolean } = $props();

  let open = $state(false);
  let active = $state(0);
  let placement = $state<Placement | null>(null);
  let triggerEl: HTMLButtonElement | undefined = $state();
  let menuEl: HTMLElement | undefined = $state();
  let addingHost = $state(false);

  /** `~/Notes` on a Host, once asked, and which Host it's for. */
  let home = $state<{ host: string; folder: string } | null>(null);
  const defaultFolder = $derived(home?.host === notes.host ? home.folder : null);
  /** Typing a folder by hand: for a Host on another machine, which this one can't browse. */
  let typing = $state<string | null>(null);

  $effect(() => {
    const host = notes.host;
    if (!hosts.reachable(host)) return;
    let stale = false;
    api.notesFolder(host).then(
      (f) => !stale && (home = { host, folder: f.default }),
      () => {},
    );
    return () => (stale = true);
  });

  const hostName = $derived(hosts.label(notes.host));
  const problem = $derived(notes.loaded && notes.problem ? hosts.problem(notes.host) : null);
  const folder = $derived(notes.folder ? fromHome(notes.folder, defaultFolder) : "…");

  type Row = {
    label: string;
    note?: string;
    /** A Host row: its dot, and whether it's the one in use. */
    host?: string;
    disabled?: boolean;
    run: () => void;
  };

  const hostRows = $derived(
    notesHostChoices(
      hosts.list.map((h) => h.id),
      notes.host,
      (id) => hosts.label(id),
      (id) => hosts.problem(id),
    ).map(
      (c): Row => ({
        label: c.name,
        note: c.note ?? (c.id === hosts.own ? "this machine" : undefined),
        host: c.id,
        disabled: c.disabled && c.id !== notes.host,
        run: () => useHost(c.id),
      }),
    ),
  );

  const folderRows = $derived.by((): Row[] => {
    const out: Row[] = [{ label: "Change folder…", run: change }];
    if (defaultFolder && defaultFolder !== notes.folder) {
      out.push({ label: "Use ~/Notes", note: "default", run: () => use(null) });
    }
    return out;
  });

  const addRow: Row = { label: "Add another machine…", run: addHost };

  /** Every row, in the order the arrows walk them. */
  const rows = $derived([...hostRows, addRow, ...folderRows]);

  function show() {
    if (!triggerEl) return;
    placement = placeMenu(triggerEl, { minWidth: 260, maxHeight: 480, align: "left" });
    active = Math.max(0, hostRows.findIndex((r) => r.host === notes.host));
    typing = null;
    open = true;
  }

  function close() {
    open = false;
    typing = null;
  }

  async function useHost(id: string) {
    close();
    await notes.useHost(id);
  }

  function addHost() {
    close();
    addingHost = true;
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
      const row = rows[active];
      if (row && !row.disabled) row.run();
    } else if (e.key === "Tab") {
      close();
    }
  }
</script>

{#snippet row(r: Row, i: number)}
  <div
    class="menu-item"
    role={r.host ? "menuitemradio" : "menuitem"}
    aria-checked={r.host ? r.host === notes.host : undefined}
    aria-disabled={r.disabled}
    tabindex="-1"
    data-active={i === active}
    onpointermove={() => (active = i)}
    onclick={() => !r.disabled && r.run()}
    onkeydown={() => {}}
  >
    {#if r.host}
      <span class="menu-tick" aria-hidden="true">{r.host === notes.host ? "✓" : ""}</span>
      <span class={`status-dot status-${hosts.dot(r.host)}`} aria-hidden="true"></span>
    {/if}
    <span class="menu-label">{r.label}</span>
    {#if r.note}<span class="menu-note">{r.note}</span>{/if}
  </div>
{/snippet}

<button
  bind:this={triggerEl}
  class="place"
  class:phone
  aria-haspopup="menu"
  aria-expanded={open}
  aria-label={`Notes on ${hostName}, in ${notes.folder || "their folder"}. Change where notes are kept`}
  title={notes.folder ? `${hostName}: ${notes.folder}` : hostName}
  onclick={() => (open ? close() : show())}
>
  <svg class="machine" viewBox="0 0 16 16" aria-hidden="true">
    <rect x="1.75" y="2.5" width="12.5" height="8.5" rx="1.3" />
    <path d="M5.5 13.75h5M8 11v2.75" />
  </svg>
  <span class="lines">
    <span class="host">
      <span class={`status-dot status-${hosts.dot(notes.host)}`} aria-hidden="true"></span>
      <span class="name">{hostName}</span>
    </span>
    {#if problem}
      <span class="problem">{problem}</span>
    {:else}
      <span class="folder"><bdi>{folder}</bdi></span>
    {/if}
  </span>
  <span class="chevron" aria-hidden="true">
    <svg viewBox="0 0 10 6" width="10" height="6">
      <path
        d="M1 1l4 4 4-4"
        fill="none"
        stroke="currentColor"
        stroke-width="1.5"
        stroke-linecap="round"
        stroke-linejoin="round"
      />
    </svg>
  </span>
</button>

{#if open && placement}
  <div
    bind:this={menuEl}
    class="popover menu"
    role="menu"
    aria-label="Where notes are kept"
    tabindex="-1"
    style={menuStyle(placement)}
    onkeydown={keydown}
  >
    <div class="heading field-label">Keep notes on</div>
    {#each hostRows as r, i (r.host)}
      {@render row(r, i)}
    {/each}
    {@render row(addRow, hostRows.length)}

    <div class="menu-sep"></div>
    <div class="where">
      <span class="field-label">Folder on {hostName}</span>
      <code title={notes.folder}>{notes.folder || "…"}</code>
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
    {#each folderRows as r, i (r.label)}
      {@render row(r, hostRows.length + 1 + i)}
    {/each}
  </div>
{/if}

{#if addingHost}
  <HostsDialog onclose={() => (addingHost = false)} />
{/if}

<style>
  /* A card rather than a word in the header: which machine the notes are on
     changes everything the list shows, so it is read at a glance. */
  .place {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    margin: 0 var(--pad-x) var(--space-3);
    padding: 0.4rem 0.6rem;
    background: transparent;
    border: var(--border-width) solid var(--border);
    border-radius: var(--radius-lg);
    color: var(--fg);
    text-align: left;
    cursor: pointer;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .place:hover,
  .place[aria-expanded="true"] {
    background: var(--hover);
  }

  .place.phone {
    min-height: 2.75rem;
  }

  .machine {
    flex: none;
    width: 1.05rem;
    height: 1.05rem;
    fill: none;
    stroke: var(--fg-muted);
    stroke-width: 1.3;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .lines {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
  }

  .host {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
  }

  .name {
    min-width: 0;
    font-size: var(--text-md);
    font-weight: var(--weight-semibold);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* The path's end is the part that tells folders apart, so it is the start
     that gives way when the pane is narrow. */
  .folder {
    font-family: var(--font-mono);
    font-size: var(--text-2xs);
    color: var(--fg-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    direction: rtl;
    text-align: left;
  }

  .problem {
    font-size: var(--text-2xs);
    color: var(--warning-text);
    overflow-wrap: anywhere;
  }

  .chevron {
    flex: none;
    display: inline-flex;
    color: var(--fg-muted);
  }

  .heading {
    padding: var(--space-2) var(--menu-item-pad-x) var(--space-2);
  }

  .where {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-2) var(--menu-item-pad-x) var(--space-2);
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
