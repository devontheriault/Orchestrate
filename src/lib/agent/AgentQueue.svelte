<script lang="ts">
  import { store } from "$lib/state/store.svelte";
  import { models } from "$lib/state/models.svelte";
  import { DEFAULT_MODE, modeLabel } from "$lib/picks";
  import Attachments from "./Attachments.svelte";

  let {
    agentId,
    /** Whether the agent is mid-Turn — a held queue reads differently. */
    running,
  }: { agentId: string; running: boolean } = $props();

  const queued = $derived(store.queue.for(agentId));

  /**
   * Collapsed by default: the strip's job is to say *that* something is
   * waiting. Reading the prompts is a second, deliberate click.
   */
  let expanded = $state(false);

  // A different agent is a different queue: start it closed like any other.
  $effect(() => {
    agentId;
    expanded = false;
  });

  // An emptied queue shouldn't reopen expanded the next time one fills.
  $effect(() => {
    if (queued.length === 0) expanded = false;
  });
</script>

{#if queued.length > 0}
  <div class="queue" class:held={!running}>
    <div class="head">
      <button
        class="toggle"
        onclick={() => (expanded = !expanded)}
        aria-expanded={expanded}
        title={expanded ? "Hide queued messages" : "Show queued messages"}
      >
        <svg
          class="chevron"
          class:open={expanded}
          viewBox="0 0 16 16"
          width="12"
          height="12"
          aria-hidden="true"
        >
          <path
            d="M6 4l4 4-4 4"
            fill="none"
            stroke="currentColor"
            stroke-width="1.8"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </svg>
        <span class="count">
          {queued.length}
          {queued.length === 1 ? "message" : "messages"} queued
        </span>
      </button>
      <div class="actions">
        {#if !running}
          <!-- The Host only sends the queue after a Turn Completes, so a
               Stopped or Failed agent's queue waits for the user to say go. -->
          <button
            class="act"
            onclick={() => store.queue.sendNext(agentId)}
            disabled={store.sending}
          >
            Send next
          </button>
        {/if}
        {#if expanded}
          <button class="act danger" onclick={() => store.queue.clear(agentId)}>
            Clear all
          </button>
        {/if}
      </div>
    </div>

    {#if expanded}
      {#if !running}
        <p class="note">
          Held — the queue only sends itself when a reply finishes on its own.
          Send the next one when you're ready.
        </p>
      {/if}
      <ul>
        {#each queued as message, i (message.id)}
          <li>
            <span class="n">{i + 1}</span>
            <div class="body">
              <p class="text">{message.prompt}</p>
              {#if message.attachments?.length}
                <Attachments paths={message.attachments} />
              {/if}
            </div>
            {#if message.model}
              <span class="badge">{models.name(message.model)}</span>
            {/if}
            <!-- Only when it isn't the usual one: a queued message that will
                 plan rather than act should say so before it goes out. -->
            {#if message.permission_mode && message.permission_mode !== DEFAULT_MODE}
              <span class="badge mode">{modeLabel(message.permission_mode)}</span>
            {/if}
            <button
              class="del"
              onclick={() => store.queue.remove(agentId, message.id)}
              aria-label="Remove this queued message"
              title="Remove this queued message"
            >
              <svg viewBox="0 0 16 16" width="15" height="15" aria-hidden="true">
                <path
                  d="M3.75 3.75l8.5 8.5M12.25 3.75l-8.5 8.5"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="2.2"
                  stroke-linecap="round"
                />
              </svg>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
{/if}

<style>
  /* Reads as a shelf sitting on the input box: same radius family, same panel
     fill, and no gap-filling chrome of its own when collapsed. */
  .queue {
    border: 1px solid var(--border);
    border-radius: var(--radius-xl);
    background: var(--panel-bg);
    font-size: var(--text-xs);
    overflow: hidden;
  }

  /* An Agent that stopped mid-queue is waiting on the user, not on itself. */
  .queue.held {
    border-color: color-mix(in srgb, var(--attention) 45%, var(--border));
  }

  .head {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding: 0.28rem 0.35rem 0.28rem 0.45rem;
  }

  .toggle {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: 0.1rem 0.15rem;
    border: none;
    background: none;
    color: var(--fg-muted);
    font: inherit;
    text-align: left;
    cursor: pointer;
    border-radius: var(--radius-md);
  }

  .toggle:hover {
    color: var(--fg);
  }

  .toggle:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  .chevron {
    flex: none;
    transition: transform var(--transition-fast);
  }

  .chevron.open {
    transform: rotate(90deg);
  }

  .count {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .actions {
    flex: none;
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }

  .act {
    padding: 0.15rem 0.4rem;
    border: none;
    border-radius: var(--radius-md);
    background: none;
    color: var(--fg-muted);
    font: inherit;
    cursor: pointer;
  }

  .act:hover:not(:disabled) {
    background: var(--hover);
    color: var(--fg);
  }

  .act.danger:hover {
    color: var(--danger);
  }

  .act:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .act:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  .note {
    margin: 0;
    padding: 0 0.6rem 0.3rem;
    color: var(--fg-muted);
  }

  /* Shorter than the textarea's cap, so the two together stay under half the
     pane: the transcript above them stays the biggest thing on it. `--pane-h`
     is set by AgentPane. */
  ul {
    margin: 0;
    padding: 0 0.35rem 0.35rem;
    list-style: none;
    max-height: calc(0.2 * var(--pane-h, 100vh));
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  li {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    padding: 0.35rem 0.4rem;
    border-radius: var(--radius-lg);
    background: var(--surface);
  }

  li:hover {
    background: var(--hover);
  }

  /* Position in the line, so a queue of three reads as an order. */
  .n {
    flex: none;
    min-width: 1rem;
    color: var(--fg-muted);
    font-variant-numeric: tabular-nums;
    line-height: var(--leading-normal);
  }

  .body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .text {
    margin: 0;
    color: var(--fg);
    line-height: var(--leading-normal);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  /* Sits centred against the row rather than riding the first line of a
     multi-line prompt: the badge labels the whole message. */
  .badge {
    flex: none;
    align-self: center;
    display: inline-flex;
    align-items: center;
    padding: 0.1rem 0.35rem;
    border-radius: var(--radius-pill);
    background: var(--code-bg);
    color: var(--fg-muted);
    font-size: var(--text-2xs);
    line-height: var(--leading-none);
    white-space: nowrap;
  }

  .del {
    flex: none;
    align-self: center;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 1.5rem;
    height: 1.5rem;
    border: none;
    border-radius: var(--radius-pill);
    background: none;
    color: var(--fg);
    cursor: pointer;
  }

  .del:hover {
    background: var(--border);
    color: var(--danger);
  }

  .del:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  /* The mode is a departure from the default, so it carries the accent the
     picker gives it rather than the model badge's quiet grey. */
  .badge.mode {
    color: var(--accent);
  }

  /* On a narrow pane the badges are the first thing worth dropping. */
  @media (max-width: 560px) {
    .badge {
      display: none;
    }
  }
</style>
