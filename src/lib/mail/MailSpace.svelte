<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { fly, fade } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import FolderList from "./FolderList.svelte";
  import { mailKey, typing } from "./list";
  import { hosts } from "$lib/state/hosts.svelte";
  import { panes, MIN_DETAIL, MIN_SIDE } from "$lib/layout/panes.svelte";
  import PaneDivider from "$lib/layout/PaneDivider.svelte";
  import { space } from "$lib/spaces/space.svelte";
  import { mail } from "./mail.svelte";
  import MailSetup from "./MailSetup.svelte";
  import Reader from "./Reader.svelte";
  import ThreadList from "./ThreadList.svelte";

  /**
   * The Mail Space (ADR 0017): folders, conversations and the reading pane,
   * side by side as the box it's given allows. Wide, all three; narrower, the
   * folders slide in from a button; narrowest (a phone), one column at a time.
   */

  let width = $state(1200);
  let search: HTMLInputElement | undefined = $state();
  let drawer = $state(false);
  let sending = $state(false);
  /** In one column: whether the reading pane is showing rather than the list. */
  let reading = $state(false);

  const columns = $derived(width >= 980 ? 3 : width >= 640 ? 2 : 1);

  /** The folders' size, unrounded, so the title bar's lead segment meets their edge exactly. */
  let foldersBox = $state<readonly ResizeObserverSize[]>();

  // The title bar tops the folders, while they have a column of their own.
  $effect(() => {
    panes.lead.mail = columns === 3 ? (foldersBox?.[0]?.inlineSize ?? 0) : 0;
  });

  onMount(() => {
    void mail.start();
  });

  onDestroy(() => mail.stop());

  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      // The rail keeps Mail mounted behind the other Spaces, hidden with
      // `visibility`, which neither `offsetParent` nor `checkVisibility()`
      // notices. Its keys are only its own while it's the one showing.
      if (!mail.connected || space.current !== "mail" || e.defaultPrevented) return;
      // Keys belong to whatever field, menu or panel has them first.
      const inside = e.target instanceof Element && e.target.closest(".popover, [role=dialog]");
      if (typing(document.activeElement) || inside) return;
      const action = mailKey(e);
      if (!action) return;
      e.preventDefault();
      switch (action) {
        case "next":
          mail.step(1);
          break;
        case "prev":
          mail.step(-1);
          break;
        case "archive":
          void mail.archive();
          break;
        case "trash":
          void mail.trash();
          break;
        case "unread":
          mail.toggleUnread();
          break;
        case "search":
          search?.focus();
          search?.select();
          break;
        case "agent":
          if (mail.selected) sending = true;
          break;
        case "refresh":
          void mail.refresh();
          break;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  // Leaving a conversation in one column goes back to the list.
  $effect(() => {
    if (!mail.selected) reading = false;
  });
</script>

<div class="mail-space" bind:clientWidth={width}>
  {#if mail.status === null}
    <!-- Nothing until the Host has answered, as the rest of the app does. -->
    {#if mail.unreachable}
      <p class="unreachable">{hosts.problem(hosts.home) ?? "Waiting for the Host…"}</p>
    {/if}
  {:else if !mail.status.account}
    <MailSetup />
  {:else}
    {#if mail.error}
      <div class="banner" role="alert">
        <span>{mail.error}</span>
        <button class="btn btn-ghost btn-sm" onclick={() => mail.refresh()}>Try again</button>
        <button class="btn btn-ghost btn-sm" onclick={() => (mail.error = null)} aria-label="Dismiss">×</button>
      </div>
    {:else if mail.status.state === "error" && mail.status.error}
      <div class="banner" role="alert">
        <span>{mail.status.error}</span>
        <button class="btn btn-ghost btn-sm" onclick={() => mail.loadStatus()}>Try again</button>
      </div>
    {/if}

    <div class="columns" data-columns={columns}>
      {#if columns === 3}
        <div
          class="col folders-col"
          style:flex-basis={panes.basis("folders")}
          style:min-width="{MIN_SIDE.folders}px"
          bind:borderBoxSize={foldersBox}
        >
          <FolderList />
        </div>
        <PaneDivider
          label="Resize folders"
          min={MIN_SIDE.folders}
          minLast={MIN_DETAIL}
          onresize={(w) => panes.setSide("folders", w)}
          onreset={() => panes.setSide("folders", null)}
        />
      {/if}
      {#if columns > 1 || !reading}
        <div
          class="col list-col"
          style:flex-basis={columns > 1 ? panes.basis("threads") : undefined}
          style:min-width={columns > 1 ? `${MIN_SIDE.threads}px` : undefined}
        >
          <ThreadList
            compact={columns < 3}
            bind:search
            onfolders={() => (drawer = true)}
            onopen={() => (reading = columns === 1)}
          />
        </div>
      {/if}
      {#if columns > 1}
        <PaneDivider
          label="Resize conversations"
          ruled
          min={MIN_SIDE.threads}
          minLast={MIN_DETAIL}
          onresize={(w) => panes.setSide("threads", w)}
          onreset={() => panes.setSide("threads", null)}
        />
      {/if}
      {#if columns > 1 || reading}
        <div class="col reader-col" style:min-width={columns > 1 ? `${MIN_DETAIL}px` : undefined}>
          <Reader bind:sending onback={columns === 1 ? () => (reading = false) : undefined} />
        </div>
      {/if}
    </div>

    {#if drawer && columns < 3}
      <button class="scrim" transition:fade={{ duration: 150 }} onclick={() => (drawer = false)} aria-label="Close folders"></button>
      <div class="drawer" transition:fly={{ x: -240, duration: 200, easing: cubicOut }}>
        <FolderList onpick={() => (drawer = false)} />
      </div>
    {/if}

    {#if mail.notice}
      <div class="toast" role="status" transition:fly={{ y: 12, duration: 180 }}>{mail.notice}</div>
    {/if}
  {/if}
</div>

<style>
  .mail-space {
    position: relative;
    flex: 1;
    min-width: 0;
    min-height: 0;
    height: 100%;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--surface);
  }

  /* Each column but the reader sized by its divider, which is the seam to its
     right; past that they give up width before the reader does. */
  .columns {
    flex: 1;
    min-height: 0;
    display: flex;
  }

  .col {
    min-width: 0;
    min-height: 0;
  }

  .folders-col {
    flex: 0 1 15rem;
  }

  .list-col {
    flex: 0 1 26rem;
  }

  .columns[data-columns="2"] .list-col {
    flex-basis: max(16rem, 40%);
  }

  .reader-col,
  .columns[data-columns="1"] .list-col {
    flex: 1 1 0;
  }

  .unreachable {
    margin: auto;
    color: var(--fg-muted);
    font-size: var(--text-md);
  }

  .banner {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: 0.45rem var(--pad-x);
    background: var(--warning-soft-bg);
    border-bottom: 1px solid var(--warning-soft-border);
    font-size: var(--text-sm);
    overflow-wrap: anywhere;
  }

  .banner span {
    flex: 1;
  }

  .scrim {
    position: absolute;
    inset: 0;
    border: none;
    background: var(--scrim);
    z-index: var(--z-overlay);
    cursor: default;
  }

  .drawer {
    position: absolute;
    top: 0;
    bottom: 0;
    left: 0;
    width: min(17rem, 85%);
    z-index: var(--z-overlay);
    box-shadow: var(--shadow-modal);
  }

  /* Floats over the conversations, so under a see-through theme it needs a
     fill of its own (see `--float-bg`) or their lines read through its rows.
     On a layer of its own: a backdrop filter on the drawer would make it the
     box its account menu, a fixed popover, is placed in. */
  .drawer::before {
    content: "";
    position: absolute;
    inset: 0;
    z-index: -1;
    background: var(--float-bg, var(--surface));
    -webkit-backdrop-filter: var(--float-filter, none);
    backdrop-filter: var(--float-filter, none);
  }

  .toast {
    position: absolute;
    left: 50%;
    bottom: calc(var(--space-6) + var(--safe-bottom));
    transform: translateX(-50%);
    padding: 0.5rem 0.9rem;
    border-radius: var(--radius-pill);
    background: var(--fg);
    color: var(--surface);
    font-size: var(--text-sm);
    box-shadow: var(--shadow-popover);
    z-index: var(--z-popover);
    pointer-events: none;
  }
</style>
