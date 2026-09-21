<script lang="ts">
  import { store } from "./store.svelte";

  let {
    agentId,
    /** Whether the agent is mid-Turn — a held queue reads differently. */
    running,
  }: { agentId: string; running: boolean } = $props();

  const queued = $derived(store.queueFor(agentId));

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
          <!-- The queue only sends itself after a Turn ends cleanly, so a
               Stopped or Failed agent's queue waits for the user to say go. -->
          <button
            class="act"
            onclick={() => store.drainQueue(agentId)}
            disabled={store.sending}
          >
            Send next
          </button>
        {/if}
        {#if expanded}
          <button class="act danger" onclick={() => store.clearQueue(agentId)}>
            Clear all
          </button>
        {/if}
      </div>
    </div>

    {#if expanded}
      {#if !running}
        <p class="note">
          Held — the last turn didn't finish on its own. Send the next one when
          you're ready.
        </p>
      {/if}
      <ul>
        {#each queued as message, i (message.id)}
          <li>
            <span class="n">{i + 1}</span>
            <p class="text">{message.prompt}</p>
            {#if message.model}
              <span class="model">{store.modelName(message.model)}</span>
            {/if}
            <button
              class="del"
              onclick={() => store.unqueue(agentId, message.id)}
              aria-label="Remove this queued message"
              title="Remove this queued message"
            >
              <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
                <path
                  d="M4.5 4.5l7 7M11.5 4.5l-7 7"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="1.8"
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
    border-radius: 12px;
    background: var(--panel-bg);
    font-size: 0.74rem;
    overflow: hidden;
  }

  /* An Agent that stopped mid-queue is waiting on the user, not on itself. */
  .queue.held {
    border-color: color-mix(in srgb, var(--attention) 45%, var(--border));
  }

  .head {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.28rem 0.35rem 0.28rem 0.45rem;
  }

  .toggle {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.1rem 0.15rem;
    border: none;
    background: none;
    color: var(--fg-muted);
    font: inherit;
    text-align: left;
    cursor: pointer;
    border-radius: 6px;
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
    transition: transform 0.12s ease;
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
    gap: 0.15rem;
  }

  .act {
    padding: 0.15rem 0.4rem;
    border: none;
    border-radius: 6px;
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
    color: #ef4444;
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

  /* The list stops growing where the textarea does: the transcript above it
     stays the biggest thing on the page. */
  ul {
    margin: 0;
    padding: 0 0.35rem 0.35rem;
    list-style: none;
    max-height: 30vh;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
  }

  li {
    display: flex;
    align-items: flex-start;
    gap: 0.45rem;
    padding: 0.35rem 0.4rem;
    border-radius: 8px;
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
    line-height: 1.5;
  }

  .text {
    flex: 1;
    min-width: 0;
    margin: 0;
    color: var(--fg);
    line-height: 1.5;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .model {
    flex: none;
    padding: 0.05rem 0.3rem;
    border-radius: 999px;
    background: var(--code-bg);
    color: var(--fg-muted);
    font-size: 0.68rem;
    white-space: nowrap;
  }

  .del {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 1.15rem;
    height: 1.15rem;
    border: none;
    border-radius: 999px;
    background: none;
    color: var(--fg-muted);
    cursor: pointer;
  }

  .del:hover {
    background: var(--border);
    color: #ef4444;
  }

  .del:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  /* On a narrow pane the model badge is the first thing worth dropping. */
  @media (max-width: 560px) {
    .model {
      display: none;
    }
  }
</style>
