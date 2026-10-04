<script lang="ts">
  import { dismissOnMove, menuStyle, placeMenu, stepActive, type Placement } from "$lib/menus/menu";
  import { folderIcon } from "./list";
  import { mail } from "./mail.svelte";
  import MailIcon from "./MailIcon.svelte";

  /** Called after a folder is picked, for a layout that shows one column. */
  let { onpick }: { onpick?: () => void } = $props();

  /** Folders with a role first, then the user's own, with a gap between. */
  const special = $derived(mail.folders.filter((f) => f.role));
  const own = $derived(mail.folders.filter((f) => !f.role));

  function pick(path: string) {
    void mail.selectFolder(path);
    onpick?.();
  }

  // The account menu: refresh and sign out.
  let menuOpen = $state(false);
  let placement = $state<Placement | null>(null);
  let active = $state(0);
  let trigger: HTMLButtonElement | undefined = $state();
  let menuEl: HTMLElement | undefined = $state();
  const items = [
    { label: "Check for new mail", run: () => mail.refresh() },
    { label: "Sign out of this account", run: () => mail.signOut(), danger: true },
  ];

  function toggleMenu() {
    if (menuOpen) {
      menuOpen = false;
      return;
    }
    placement = placeMenu(trigger!, { minWidth: 220, maxHeight: 200, align: "left" });
    active = 0;
    menuOpen = true;
  }

  $effect(() => {
    if (!menuOpen) return;
    menuEl?.focus();
    return dismissOnMove(() => [trigger, menuEl], () => (menuOpen = false));
  });

  function menuKey(e: KeyboardEvent) {
    const next = stepActive(e.key, active, items.length);
    if (next !== null) {
      e.preventDefault();
      active = next;
    } else if (e.key === "Enter") {
      e.preventDefault();
      menuOpen = false;
      void items[active].run();
    } else if (e.key === "Escape") {
      menuOpen = false;
      trigger?.focus();
    }
  }
</script>

<nav class="folders" aria-label="Mail folders">
  <header>
    <span class="title">Mail</span>
  </header>

  <ul>
    {#each special as f (f.path)}
      {@render row(f)}
    {/each}
    {#if own.length}
      <li class="gap" aria-hidden="true"></li>
      {#each own as f (f.path)}
        {@render row(f)}
      {/each}
    {/if}
  </ul>

  <footer>
    <button
      class="account"
      bind:this={trigger}
      onclick={toggleMenu}
      aria-haspopup="menu"
      aria-expanded={menuOpen}
      title={mail.status?.account?.server}
    >
      <span class="dot" class:offline={mail.status?.state === "error"}></span>
      <span class="who">{mail.me}</span>
      <MailIcon name="more" />
    </button>
  </footer>
</nav>

{#snippet row(f: (typeof mail.folders)[number])}
  <li>
    <button
      class="folder"
      class:current={f.path === mail.folder && !mail.results}
      style:padding-left="calc(var(--pad-x) + {f.depth * 0.9}rem)"
      onclick={() => pick(f.path)}
      aria-current={f.path === mail.folder && !mail.results ? "page" : undefined}
    >
      <MailIcon name={folderIcon(f)} />
      <span class="name">{f.name}</span>
      {#if f.unread > 0 && f.role !== "sent" && f.role !== "trash" && f.role !== "all"}
        <span class="count">{f.unread}</span>
      {/if}
    </button>
  </li>
{/snippet}

{#if menuOpen && placement}
  <div
    class="popover menu"
    role="menu"
    tabindex="-1"
    bind:this={menuEl}
    style={menuStyle(placement)}
    onkeydown={menuKey}
  >
    {#each items as item, i}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div
        class="menu-item"
        class:danger={item.danger}
        role="menuitem"
        tabindex="-1"
        data-active={i === active}
        onpointermove={() => (active = i)}
        onclick={() => {
          menuOpen = false;
          void item.run();
        }}
      >
        {item.label}
      </div>
    {/each}
  </div>
{/if}

<style>
  .folders {
    display: flex;
    flex-direction: column;
    min-height: 0;
    height: 100%;
    background: var(--panel-bg);
  }

  header {
    padding: var(--pad-y) var(--pad-x);
    min-height: 2.8rem;
    display: flex;
    align-items: center;
  }

  .title {
    font-size: var(--text-sm);
    font-weight: var(--weight-semibold);
    color: var(--fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0 0 var(--space-3);
    overflow-y: auto;
    flex: 1;
    min-height: 0;
  }

  .gap {
    height: var(--space-5);
  }

  .folder {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding: 0.42rem var(--pad-x);
    border: none;
    background: transparent;
    color: var(--fg);
    font: inherit;
    font-size: var(--text-lg);
    text-align: left;
    cursor: pointer;
  }

  .folder :global(.icon) {
    color: var(--fg-muted);
    width: 1.05em;
    height: 1.05em;
  }

  .folder:hover {
    background: var(--hover);
  }

  .folder.current {
    background: var(--selected);
    font-weight: var(--weight-semibold);
  }

  .folder.current :global(.icon) {
    color: var(--accent);
  }

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .count {
    font-size: var(--text-xs);
    font-weight: var(--weight-semibold);
    color: var(--accent);
    font-variant-numeric: tabular-nums;
  }

  footer {
    flex: none;
    padding: var(--space-3);
    border-top: 1px solid var(--border);
  }

  .account {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding: 0.4rem 0.5rem;
    border: none;
    border-radius: var(--radius-md);
    background: transparent;
    color: var(--fg-muted);
    font: inherit;
    font-size: var(--text-sm);
    cursor: pointer;
  }

  .account:hover {
    background: var(--hover);
    color: var(--fg);
  }

  .who {
    flex: 1;
    min-width: 0;
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dot {
    width: 0.5rem;
    height: 0.5rem;
    border-radius: var(--radius-circle);
    background: var(--success);
    flex: none;
  }

  .dot.offline {
    background: var(--warning);
  }

  .menu-item.danger {
    color: var(--danger-text);
  }
</style>
