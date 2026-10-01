<script lang="ts">
  /**
   * The app's `/plugin`: Claude Code's plugin screen as a window. Discover
   * lists what the marketplaces offer, Installed what's on this machine, and
   * Marketplaces where the plugins come from. Every change goes through
   * `claude plugin`, and reaches each agent from its next prompt, since every
   * prompt starts a fresh `claude`.
   */
  import { hosts } from "$lib/state/hosts.svelte";
  import type { AvailablePlugin, InstalledPlugin } from "$lib/api";
  import { plugins } from "./plugins.svelte";
  import { installsLabel, matchPlugins, splitId, type Tab } from "./catalog";

  /** Anthropic's own marketplace, offered when there's none to browse. */
  const OFFICIAL = "anthropics/claude-plugins-official";

  const TABS: { id: Tab; label: string }[] = [
    { id: "discover", label: "Discover" },
    { id: "installed", label: "Installed" },
    { id: "marketplaces", label: "Marketplaces" },
  ];

  const catalog = $derived(plugins.catalog);
  const counts = $derived<Record<Tab, number | null>>({
    discover: catalog?.available.length ?? null,
    installed: catalog?.installed.length ?? null,
    marketplaces: catalog?.marketplaces.length ?? null,
  });
  const discovered = $derived(matchPlugins(catalog?.available ?? [], plugins.query));
  const installed = $derived(matchPlugins(catalog?.installed ?? [], plugins.query));

  /** The marketplace a user is adding, as typed. */
  let source = $state("");
  let searchEl: HTMLInputElement | undefined = $state();
  let sourceEl: HTMLInputElement | undefined = $state();
  let bodyEl: HTMLElement | undefined = $state();

  // The keys go where the tab's work is: the search, or the marketplace box.
  $effect(() => {
    const tab = plugins.tab;
    requestAnimationFrame(() => (tab === "marketplaces" ? sourceEl : searchEl)?.focus());
  });

  // Each tab starts at its top, not wherever the last one was scrolled to.
  $effect(() => {
    plugins.tab;
    plugins.query;
    bodyEl?.scrollTo({ top: 0 });
  });

  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      // Esc backs out of the question first, then out of the window.
      if (plugins.confirming) plugins.confirming = null;
      else plugins.close();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  /** Left and right walk the tabs, as a tab list's keys do. */
  function onTabKey(e: KeyboardEvent) {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    e.preventDefault();
    const i = TABS.findIndex((t) => t.id === plugins.tab);
    const next = (i + (e.key === "ArrowRight" ? 1 : TABS.length - 1)) % TABS.length;
    plugins.tab = TABS[next].id;
    document.getElementById(`plugins-tab-${TABS[next].id}`)?.focus();
  }

  function install(p: AvailablePlugin) {
    plugins.act(p.id, "Installing", { do: "install", plugin: p.id });
  }

  function toggle(p: InstalledPlugin) {
    const change = p.enabled ? "disable" : "enable";
    plugins.act(p.id, p.enabled ? "Disabling" : "Enabling", {
      do: change,
      plugin: p.id,
      scope: p.scope,
    });
  }

  function addMarketplace(e: SubmitEvent) {
    e.preventDefault();
    const typed = source.trim();
    if (!typed) return;
    plugins.act(typed, "Adding", { do: "add_marketplace", source: typed }).then(() => {
      if (!plugins.notice?.failed) source = "";
    });
  }

  function addOfficial() {
    plugins.tab = "marketplaces";
    plugins.act(OFFICIAL, "Adding", { do: "add_marketplace", source: OFFICIAL });
  }

  const where = $derived(hosts.several ? hosts.label(plugins.host) : null);
</script>

<!-- Closes on a click outside the dialog, not on one that lands inside it. -->
<div
  class="backdrop"
  role="presentation"
  onclick={(e) => {
    if (e.target === e.currentTarget) plugins.close();
  }}
>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="plugins-title" tabindex="-1">
    <header>
      <div class="title">
        <h2 id="plugins-title">Plugins</h2>
        {#if where}<span class="where" title="Plugins belong to a machine">on {where}</span>{/if}
        <button class="btn btn-ghost btn-icon close" onclick={() => plugins.close()} aria-label="Close">×</button>
      </div>

      <div class="tabs" role="tablist" aria-label="Plugin lists">
        {#each TABS as t (t.id)}
          <button
            id={`plugins-tab-${t.id}`}
            role="tab"
            class="tab"
            aria-selected={plugins.tab === t.id}
            aria-controls="plugins-panel"
            tabindex={plugins.tab === t.id ? 0 : -1}
            onclick={() => (plugins.tab = t.id)}
            onkeydown={onTabKey}
          >
            {t.label}
            {#if counts[t.id] !== null}<span class="count">{counts[t.id]}</span>{/if}
          </button>
        {/each}
      </div>

      {#if plugins.tab === "marketplaces"}
        <form class="bar" onsubmit={addMarketplace}>
          <input
            bind:this={sourceEl}
            bind:value={source}
            class="input input-mono"
            placeholder="owner/repo, a git URL, or a path"
            aria-label="Marketplace to add"
            spellcheck="false"
            autocomplete="off"
          />
          <button class="btn btn-primary" disabled={!source.trim() || !!plugins.busy[source.trim()]}>
            {plugins.busy[source.trim()] ? "Adding…" : "Add"}
          </button>
        </form>
      {:else}
        <div class="bar search">
          <svg class="glass" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
            <circle cx="7" cy="7" r="4.75" fill="none" stroke="currentColor" stroke-width="1.5" />
            <path d="M10.5 10.5 14 14" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
          </svg>
          <input
            bind:this={searchEl}
            bind:value={plugins.query}
            class="input"
            type="search"
            placeholder={plugins.tab === "discover" ? "Search plugins" : "Search installed plugins"}
            aria-label="Search plugins"
            spellcheck="false"
            autocomplete="off"
          />
        </div>
      {/if}
    </header>

    <div class="body" id="plugins-panel" role="tabpanel" aria-labelledby={`plugins-tab-${plugins.tab}`} bind:this={bodyEl}>
      {#if plugins.confirming}
        {@const held = plugins.confirming}
        <div class="confirm" role="alertdialog" aria-labelledby="plugins-confirm-title">
          <p id="plugins-confirm-title">
            <strong>{splitId(held.key).name}</strong> installs by running a command its marketplace
            declares. Run it only if you trust where it comes from.
          </p>
          <pre>{held.command}</pre>
          <div class="confirm-actions">
            <button class="btn" onclick={() => (plugins.confirming = null)}>Cancel</button>
            <button
              class="btn btn-warning"
              onclick={async () => {
                const said = await plugins.accept();
                if (said) plugins.notice = said;
              }}
            >
              Run it and install
            </button>
          </div>
        </div>
      {/if}

      {#if plugins.error}
        <div class="err">
          {plugins.error}
          <button class="btn btn-sm" onclick={() => plugins.load(true)}>Try again</button>
        </div>
      {/if}

      {#if !catalog}
        {#if plugins.loading}
          <p class="empty">Asking Claude Code for its plugins…</p>
        {/if}
      {:else if plugins.tab === "discover"}
        {#if !catalog.marketplaces.length}
          <div class="empty">
            <p>There are no marketplaces to browse yet. A marketplace is a catalog of plugins.</p>
            <button class="btn btn-primary" disabled={!!plugins.busy[OFFICIAL]} onclick={addOfficial}>
              {plugins.busy[OFFICIAL] ? "Adding…" : "Add Anthropic's marketplace"}
            </button>
          </div>
        {:else if !discovered.length}
          <p class="empty">
            {plugins.query.trim()
              ? `No plugin on offer matches “${plugins.query.trim()}”.`
              : "Every plugin on offer is installed."}
          </p>
        {:else}
          <ul class="list">
            {#each discovered as p (p.id)}
              {@const doing = plugins.busy[p.id]}
              <li class="plugin">
                <div class="head">
                  <span class="name">{p.name}</span>
                  <span class="from">{p.marketplace}</span>
                  {#if p.installs !== null}<span class="installs">{installsLabel(p.installs)}</span>{/if}
                </div>
                {#if p.description}<p class="desc">{p.description}</p>{/if}
                <div class="actions">
                  <button class="btn btn-sm" class:btn-primary={!doing} disabled={!!doing} onclick={() => install(p)}>
                    {doing ? `${doing}…` : "Install"}
                  </button>
                </div>
              </li>
            {/each}
          </ul>
        {/if}
      {:else if plugins.tab === "installed"}
        {#if !installed.length}
          <div class="empty">
            {#if plugins.query.trim()}
              <p>No installed plugin matches “{plugins.query.trim()}”.</p>
            {:else}
              <p>No plugins installed yet.</p>
              <button class="btn" onclick={() => (plugins.tab = "discover")}>Discover plugins</button>
            {/if}
          </div>
        {:else}
          <ul class="list">
            {#each installed as p (p.id)}
              {@const id = splitId(p.id)}
              {@const doing = plugins.busy[p.id]}
              <li class="plugin" class:off={!p.enabled}>
                <div class="head">
                  <span class="name">{id.name}</span>
                  <span class="from">{id.marketplace}</span>
                  {#if p.version}<span class="version">{p.version}</span>{/if}
                  {#if p.scope !== "user"}
                    <span class="scope" title={`Installed in ${p.scope} scope`}>{p.scope}</span>
                  {/if}
                </div>
                {#if p.description}<p class="desc">{p.description}</p>{/if}
                <div class="actions">
                  {#if doing}<span class="doing">{doing}…</span>{/if}
                  <button class="btn btn-ghost btn-sm" disabled={!!doing} onclick={() => plugins.act(p.id, "Updating", { do: "update", plugin: p.id, scope: p.scope })}>
                    Update
                  </button>
                  <button class="btn btn-ghost btn-sm danger" disabled={!!doing} onclick={() => plugins.act(p.id, "Uninstalling", { do: "uninstall", plugin: p.id, scope: p.scope })}>
                    Uninstall
                  </button>
                  <label class="switch" title={p.enabled ? "Enabled: agents load it" : "Disabled: installed, but agents don't load it"}>
                    <input type="checkbox" role="switch" checked={p.enabled} disabled={!!doing} onchange={() => toggle(p)} aria-label={`Enable ${id.name}`} />
                    <span class="track" aria-hidden="true"><span class="thumb"></span></span>
                  </label>
                </div>
              </li>
            {/each}
          </ul>
        {/if}
      {:else}
        {#if !catalog.marketplaces.length}
          <div class="empty">
            <p>No marketplaces yet. Add one above, or start with Anthropic's own.</p>
            <button class="btn btn-primary" disabled={!!plugins.busy[OFFICIAL]} onclick={addOfficial}>
              {plugins.busy[OFFICIAL] ? "Adding…" : `Add ${OFFICIAL}`}
            </button>
          </div>
        {:else}
          <ul class="list">
            {#each catalog.marketplaces as m (m.name)}
              {@const doing = plugins.busy[m.name]}
              {@const offered = catalog.available.filter((p) => p.marketplace === m.name).length}
              {@const have = catalog.installed.filter((p) => splitId(p.id).marketplace === m.name).length}
              <li class="plugin">
                <div class="head">
                  <span class="name">{m.name}</span>
                  <span class="installs">{have} installed · {offered} more on offer</span>
                </div>
                <p class="desc mono">{m.source}{m.location ? ` · ${m.location}` : ""}</p>
                <div class="actions">
                  {#if doing}<span class="doing">{doing}…</span>{/if}
                  <button class="btn btn-ghost btn-sm" disabled={!!doing} onclick={() => plugins.act(m.name, "Updating", { do: "update_marketplace", name: m.name })}>
                    Update
                  </button>
                  <button class="btn btn-ghost btn-sm danger" disabled={!!doing} onclick={() => plugins.act(m.name, "Removing", { do: "remove_marketplace", name: m.name })}>
                    Remove
                  </button>
                </div>
              </li>
            {/each}
          </ul>
        {/if}
      {/if}
    </div>

    <footer>
      {#if plugins.notice}
        <span class="notice" class:failed={plugins.notice.failed} role="status">{plugins.notice.text}</span>
      {:else}
        <span class="hint">Agents pick up changes from their next prompt<span class="keys-only"> · Esc to close</span></span>
      {/if}
      <button class="btn btn-sm" onclick={() => plugins.load(true)} disabled={plugins.loading}>
        {plugins.loading && catalog ? "Refreshing…" : "Refresh"}
      </button>
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: var(--scrim);
    display: flex;
    align-items: center;
    justify-content: center;
    --gutter: var(--space-6);
    padding: var(--gutter);
    /* Below the title bar, which sits over the scrim (see UsageWindow). */
    padding-top: calc(max(var(--titlebar-h), 28px) + var(--safe-top) + var(--gutter));
    /* And on a phone, clear of the home indicator. */
    padding-bottom: calc(var(--gutter) + var(--safe-bottom));
    z-index: var(--z-overlay);
    backdrop-filter: blur(2px);
  }

  /* A phone spends its narrow screen on the window, not on scrim round it. */
  :global(html[data-frame="mobile"]) .backdrop {
    --gutter: var(--space-3);
  }

  /* A fixed height, so switching tabs or searching doesn't make it jump. */
  .dialog {
    background: var(--float-bg, var(--surface));
    -webkit-backdrop-filter: var(--float-filter, none);
    backdrop-filter: var(--float-filter, none);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    width: min(44rem, 100%);
    height: min(40rem, 100%);
    box-shadow: var(--shadow-modal);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  header {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: 1rem 1.25rem 0.85rem;
    border-bottom: 1px solid var(--border);
  }

  .title {
    display: flex;
    align-items: baseline;
    gap: var(--space-4);
  }

  h2 {
    font-size: var(--text-2xl);
    margin: 0;
    font-weight: var(--weight-semibold);
  }

  .where {
    font-size: var(--text-xs);
    color: var(--fg-muted);
  }

  .close {
    margin-left: auto;
    align-self: center;
    font-size: var(--text-2xl);
    line-height: var(--leading-none);
  }

  .tabs {
    display: flex;
    gap: var(--space-2);
  }

  .tab {
    display: inline-flex;
    align-items: center;
    gap: var(--space-3);
    padding: 0.3rem 0.7rem;
    border: 1px solid transparent;
    border-radius: var(--radius-pill);
    background: transparent;
    color: var(--fg-muted);
    font-size: var(--text-sm);
    font-weight: var(--weight-medium);
    cursor: pointer;
    transition:
      background var(--transition-fast),
      color var(--transition-fast);
  }

  .tab:hover {
    background: var(--hover);
    color: var(--fg);
  }

  .tab[aria-selected="true"] {
    background: var(--selected);
    border-color: var(--border);
    color: var(--fg);
  }

  .tab:focus-visible {
    outline: none;
    box-shadow: var(--focus-ring);
  }

  .count {
    font-size: var(--text-2xs);
    font-variant-numeric: tabular-nums;
    color: var(--fg-muted);
  }

  .bar {
    display: flex;
    gap: var(--space-3);
  }

  .search {
    position: relative;
  }

  .glass {
    position: absolute;
    left: 0.6rem;
    top: 50%;
    transform: translateY(-50%);
    color: var(--fg-muted);
    pointer-events: none;
  }

  .search .input {
    padding-left: 1.9rem;
  }

  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
    padding: 0.75rem 1.25rem 1rem;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--panel-bg);
  }

  /* Name and what it does on the left, what can be done to it on the right. */
  .plugin {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    grid-template-areas:
      "head actions"
      "desc actions";
    column-gap: var(--space-5);
    row-gap: var(--space-1);
    align-items: center;
    padding: 0.6rem 0.8rem;
  }

  .plugin + .plugin {
    border-top: 1px solid var(--border);
  }

  .plugin.off .name,
  .plugin.off .desc {
    opacity: 0.55;
  }

  .head {
    grid-area: head;
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    gap: 0.1rem var(--space-3);
    min-width: 0;
  }

  .name {
    font-size: var(--text-md);
    font-weight: var(--weight-semibold);
    overflow-wrap: anywhere;
  }

  .from,
  .version,
  .installs {
    font-size: var(--text-xs);
    color: var(--fg-muted);
  }

  .version {
    font-family: var(--font-mono);
  }

  .installs {
    font-variant-numeric: tabular-nums;
  }

  .scope {
    padding: 0 0.3rem;
    border-radius: var(--radius-xs);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    color: var(--accent);
    font-size: var(--text-3xs);
    font-weight: var(--weight-medium);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  /* Two lines of what it does: enough to choose by, and the list stays scannable. */
  .desc {
    grid-area: desc;
    margin: 0;
    font-size: var(--text-sm);
    color: var(--fg-muted);
    line-height: var(--leading-snug);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .desc.mono {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    overflow-wrap: anywhere;
  }

  .actions {
    grid-area: actions;
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .doing {
    font-size: var(--text-xs);
    color: var(--fg-muted);
    margin-right: var(--space-2);
  }

  .danger:hover:not(:disabled) {
    color: var(--danger-text);
  }

  /* On and off, the way Claude Code's own list toggles a plugin. */
  .switch {
    margin-left: var(--space-3);
  }

  .confirm {
    border: 1px solid var(--warning);
    border-radius: var(--radius-md);
    background: color-mix(in srgb, var(--warning) 10%, var(--surface));
    padding: 0.75rem 0.9rem;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    font-size: var(--text-sm);
  }

  .confirm p {
    margin: 0;
  }

  .confirm pre {
    margin: 0;
    padding: 0.5rem 0.65rem;
    border-radius: var(--radius-sm);
    background: var(--code-bg);
    font-size: var(--text-xs);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .confirm-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-3);
  }

  .empty {
    margin: 0;
    padding: 2rem 1rem;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-4);
    text-align: center;
    font-size: var(--text-md);
    color: var(--fg-muted);
  }

  .empty p {
    margin: 0;
  }

  .err {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    background: var(--danger-soft-bg);
    border: 1px solid var(--danger-soft-border);
    border-radius: var(--radius-md);
    padding: 0.5rem 0.7rem;
    font-size: var(--text-sm);
    font-family: var(--font-mono);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-5);
    padding: 0.6rem 1.25rem 0.75rem;
    border-top: 1px solid var(--border);
  }

  .hint,
  .notice {
    font-size: var(--text-xs);
    color: var(--fg-muted);
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .notice {
    color: var(--fg);
  }

  .notice.failed {
    color: var(--danger-text);
  }
</style>
