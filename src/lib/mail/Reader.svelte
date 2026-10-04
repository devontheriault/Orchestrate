<script lang="ts">
  import { save } from "@tauri-apps/plugin-dialog";
  import LinkedText from "$lib/markdown/LinkedText.svelte";
  import type { MailAddress } from "$lib/api";
  import { avatarColor, bytes, displayName, fullDate, initials, when } from "./list";
  import { mail } from "./mail.svelte";
  import MailIcon from "./MailIcon.svelte";
  import MessageFrame from "./MessageFrame.svelte";
  import SendToAgent from "./SendToAgent.svelte";

  let {
    onback,
    sending = $bindable(false),
  }: {
    /** Shown in a one-column layout: back to the list. */
    onback?: () => void;
    /** The Send to Agent panel is open. Bound, so `a` can open it. */
    sending?: boolean;
  } = $props();

  let agentButton: HTMLButtonElement | undefined = $state();

  const thread = $derived(mail.selected);
  /** The open message, if it belongs to the conversation on show. */
  const open = $derived(
    mail.open && thread?.messages.some((m) => m.uid === mail.open!.uid && m.folder === mail.open!.folder)
      ? mail.open
      : null,
  );
  /** While a message loads, what the list already knows of it. */
  const coming = $derived.by(() => {
    const [folder, uid] = (mail.opening ?? "").split(/:(?=\d+$)/);
    return thread?.messages.find((m) => m.folder === folder && String(m.uid) === uid) ?? null;
  });

  // The panel is for one conversation; moving to another closes it.
  let shownId: string | null = null;
  $effect(() => {
    const id = thread?.id ?? null;
    if (id !== shownId) {
      shownId = id;
      sending = false;
    }
  });
  const openKey = $derived(open ? `${open.folder}:${open.uid}` : null);
  const showingImages = $derived(!!openKey && !!mail.imagesFor[openKey]);

  function others(to: MailAddress[], cc: MailAddress[]): string {
    const names = [...to, ...cc].map((a) => displayName(a, mail.me));
    if (names.length <= 3) return names.join(", ");
    return `${names.slice(0, 2).join(", ")} and ${names.length - 2} more`;
  }

  async function download(index: number, name: string) {
    const path = await save({ defaultPath: name, title: "Save attachment" });
    if (!path) return;
    try {
      await mail.download(index, path);
    } catch (e) {
      mail.error = String(e);
    }
  }
</script>

<article class="reader" aria-label="Message">
  {#if !thread}
    <div class="blank">
      <MailIcon name="mail" size="2.4rem" />
      <p>Pick a conversation to read it.</p>
      <p class="keys keys-only">
        <kbd>j</kbd> <kbd>k</kbd> to move, <kbd>/</kbd> to search
      </p>
    </div>
  {:else}
    <header>
      {#if onback}
        <button class="btn btn-ghost btn-icon back" onclick={onback} aria-label="Back to the list">
          <MailIcon name="back" />
        </button>
      {/if}
      <h1>{thread.subject}</h1>
      <div class="tools">
        <button class="btn btn-ghost btn-icon" onclick={() => mail.archive()} title="Archive (e)" aria-label="Archive">
          <MailIcon name="archive" />
        </button>
        <button class="btn btn-ghost btn-icon" onclick={() => mail.trash()} title="Delete (#)" aria-label="Delete">
          <MailIcon name="trash" />
        </button>
        <button
          class="btn btn-ghost btn-icon"
          onclick={() => mail.toggleUnread()}
          title={thread.unread > 0 ? "Mark as read (u)" : "Mark as unread (u)"}
          aria-label={thread.unread > 0 ? "Mark as read" : "Mark as unread"}
        >
          <MailIcon name="unread" />
        </button>
        <button
          class="btn btn-sm agent"
          class:on={sending}
          bind:this={agentButton}
          onclick={() => (sending = !sending)}
          title="Hand this conversation to a new agent (a)"
          aria-haspopup="dialog"
          aria-expanded={sending}
        >
          <MailIcon name="agent" />
          <span>Send to agent</span>
        </button>
      </div>
    </header>

    {#if thread.messages.length > 1}
      <ol class="earlier">
        {#each thread.messages as m (m.folder + m.uid)}
          {@const isOpen = open && open.uid === m.uid && open.folder === m.folder}
          <li class:current={isOpen}>
            <button onclick={() => mail.openMessage(m.folder, m.uid)} aria-current={isOpen || undefined}>
              <span class="avatar small" style:background={avatarColor(m.from?.email ?? "")}>{initials(m.from)}</span>
              <span class="from">{displayName(m.from, mail.me)}</span>
              <span class="snip">{m.snippet}</span>
              <time>{when(m.date)}</time>
            </button>
          </li>
        {/each}
      </ol>
    {/if}

    {#if open}
      <div class="message">
        <div class="meta">
          <span class="avatar" style:background={avatarColor(open.from?.email ?? "")}>{initials(open.from)}</span>
          <div class="people">
            <div class="sender">
              <strong>{displayName(open.from)}</strong>
              {#if open.from?.name}<span class="addr">{open.from.email}</span>{/if}
            </div>
            <div class="to">to {others(open.to, open.cc) || "undisclosed recipients"}</div>
          </div>
          <time>{fullDate(open.date)}</time>
        </div>

        {#if open.remote_images > 0 && !showingImages}
          <div class="images">
            <MailIcon name="shield" />
            <span>
              {open.remote_images} remote image{open.remote_images === 1 ? "" : "s"} hidden. Loading
              them tells the sender you opened this.
            </span>
            <button class="btn btn-sm" onclick={() => mail.loadImages()}>Load images</button>
          </div>
        {/if}

        <div class="body">
          {#if open.html}
            <MessageFrame html={open.html} images={showingImages} />
          {:else}
            <div class="text"><LinkedText text={open.text} /></div>
          {/if}
        </div>

        {#if open.attachments.length}
          <ul class="files" aria-label="Attachments">
            {#each open.attachments as a (a.index)}
              <li>
                <MailIcon name="clip" />
                <span class="fname" title={a.name}>{a.name}</span>
                <span class="fsize">{bytes(a.size)}</span>
                <button
                  class="btn btn-ghost btn-icon"
                  onclick={() => download(a.index, a.name)}
                  aria-label="Save {a.name}"
                  title="Save to…"
                >
                  <MailIcon name="download" />
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    {:else if coming}
      <div class="message">
        <div class="meta">
          <span class="avatar" style:background={avatarColor(coming.from?.email ?? "")}>{initials(coming.from)}</span>
          <div class="people">
            <div class="sender"><strong>{displayName(coming.from)}</strong></div>
            <div class="to">{coming.snippet}</div>
          </div>
          <time>{fullDate(coming.date)}</time>
        </div>
        <div class="body"><div class="skeleton" aria-label="Loading the message"></div></div>
      </div>
    {/if}
  {/if}
</article>

{#if sending && thread && agentButton}
  <SendToAgent trigger={agentButton} onclose={() => (sending = false)} />
{/if}

<style>
  .reader {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    height: 100%;
    background: var(--surface);
  }

  .blank {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-3);
    color: var(--fg-muted);
    font-size: var(--text-md);
  }

  .blank :global(.icon) {
    opacity: 0.45;
    stroke-width: 1.3;
  }

  .blank p {
    margin: 0;
  }

  kbd {
    font-family: var(--font-mono);
    font-size: var(--text-3xs);
    border: 1px solid var(--border);
    border-bottom-width: 2px;
    border-radius: var(--radius-xs);
    padding: 0 0.3rem;
  }

  header {
    flex: none;
    display: flex;
    align-items: flex-start;
    gap: var(--space-4);
    padding: var(--pad-y) var(--pad-x) var(--space-4);
  }

  h1 {
    flex: 1;
    min-width: 0;
    margin: 0.2rem 0 0;
    font-size: var(--text-3xl);
    font-weight: var(--weight-semibold);
    line-height: var(--leading-tight);
    overflow-wrap: anywhere;
  }

  .tools {
    flex: none;
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .tools :global(.icon) {
    width: 1rem;
    height: 1rem;
  }

  .agent {
    gap: var(--space-3);
    margin-left: var(--space-3);
  }

  .agent :global(.icon) {
    color: var(--accent);
  }

  .agent.on {
    border-color: var(--accent);
  }

  .earlier {
    flex: none;
    list-style: none;
    margin: 0;
    padding: 0 var(--pad-x) var(--space-3);
    max-height: 32%;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .earlier button {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding: 0.35rem 0.5rem;
    border: 1px solid transparent;
    border-radius: var(--radius-md);
    background: var(--panel-bg);
    color: var(--fg);
    font: inherit;
    font-size: var(--text-sm);
    cursor: pointer;
    text-align: left;
  }

  .earlier button:hover {
    background: var(--hover);
  }

  .earlier li.current button {
    border-color: var(--accent);
    background: var(--selected);
  }

  .from {
    flex: none;
    font-weight: var(--weight-medium);
    max-width: 30%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .snip {
    flex: 1;
    min-width: 0;
    color: var(--fg-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  time {
    flex: none;
    color: var(--fg-muted);
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
  }

  .message {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: var(--space-3) var(--pad-x) var(--pad-y);
  }

  .meta {
    display: flex;
    align-items: center;
    gap: var(--space-4);
  }

  .avatar {
    flex: none;
    width: 2.2rem;
    height: 2.2rem;
    border-radius: var(--radius-circle);
    display: grid;
    place-items: center;
    color: #fff;
    font-weight: var(--weight-semibold);
    font-size: var(--text-sm);
    letter-spacing: 0.02em;
  }

  .avatar.small {
    width: 1.4rem;
    height: 1.4rem;
    font-size: var(--text-3xs);
  }

  .people {
    flex: 1;
    min-width: 0;
  }

  .sender {
    display: flex;
    align-items: baseline;
    gap: var(--space-3);
    min-width: 0;
    font-size: var(--text-lg);
  }

  .addr,
  .to {
    color: var(--fg-muted);
    font-size: var(--text-sm);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .images {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding: var(--space-3) var(--space-4);
    border-radius: var(--radius-md);
    background: var(--warning-soft-bg);
    border: 1px solid var(--warning-soft-border);
    font-size: var(--text-sm);
  }

  .images span {
    flex: 1;
  }

  .images :global(.icon) {
    color: var(--warning-text);
  }

  .body {
    flex: 1;
    min-height: 12rem;
    display: flex;
    flex-direction: column;
  }

  .text {
    flex: 1;
    overflow-y: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font-size: var(--text-lg);
    line-height: var(--leading-relaxed);
    padding: var(--space-2) var(--space-1);
  }

  .files {
    flex: none;
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }

  .files li {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: 0.2rem 0.2rem 0.2rem 0.6rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--panel-bg);
    font-size: var(--text-sm);
    max-width: 22rem;
  }

  .files :global(.icon) {
    color: var(--fg-muted);
  }

  .fname {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .fsize {
    color: var(--fg-muted);
    font-size: var(--text-xs);
    flex: none;
  }

  .skeleton {
    flex: 1;
    border-radius: var(--radius-lg);
    background: linear-gradient(
      100deg,
      var(--panel-bg) 30%,
      color-mix(in srgb, var(--hover) 70%, var(--panel-bg)) 50%,
      var(--panel-bg) 70%
    );
    background-size: 300% 100%;
    animation: shimmer 1.4s ease-in-out infinite;
  }

  @keyframes shimmer {
    from {
      background-position: 100% 0;
    }
    to {
      background-position: 0 0;
    }
  }

  .back {
    margin-left: calc(var(--space-3) * -1);
  }
</style>
