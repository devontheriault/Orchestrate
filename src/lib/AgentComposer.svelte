<script lang="ts">
  import { untrack } from "svelte";
  import { store } from "./store.svelte";
  import { DEFAULT_EFFORT, DEFAULT_MODEL } from "./api";
  import ModelPicker from "./ModelPicker.svelte";
  import AgentQueue from "./AgentQueue.svelte";

  const agent = $derived(store.selectedAgent);
  /** No agent yet: this composer is holding the opening prompt for a new one. */
  const drafting = $derived(store.drafting && !agent);
  const working = $derived(agent?.state === "running");
  /**
   * A Turn is being started right now. The one state that locks the box: while
   * the agent is merely *working*, the box stays live so the user can line up
   * what to say next.
   */
  const inFlight = $derived(store.sending || store.spawning);
  /**
   * Whether what's typed now would be queued rather than sent. Needs a session
   * to resume into later, so an agent that can't be continued at all still just
   * offers Stop.
   */
  const queueing = $derived(working && !!agent?.session_id);

  let prompt = $state("");
  /** The picker's value; DEFAULT_MODEL passes no --model. */
  let model = $state(DEFAULT_MODEL);
  /** The effort chosen beside it; DEFAULT_EFFORT passes no --effort. */
  let effort = $state(DEFAULT_EFFORT);
  let textarea: HTMLTextAreaElement | undefined = $state();

  // Grow with the text, up to a ceiling — a long follow-up shouldn't need
  // scrolling, but it shouldn't swallow the transcript either.
  function fit() {
    if (!textarea) return;
    textarea.style.height = "auto";
    textarea.style.height = `${textarea.scrollHeight}px`;
  }

  $effect(() => {
    prompt;
    fit();
  });

  // A different agent — or a blank page for a new one — is a different
  // conversation: don't carry a draft over. The picker starts on whatever the
  // agent last ran on, or on the user's habitual model for a new agent —
  // untracked, so a record updating mid-edit doesn't undo a model just picked.
  $effect(() => {
    store.selectedAgentId;
    store.drafting;
    prompt = "";
    untrack(() => {
      const fresh = store.drafting && !store.selectedAgent;
      model = fresh
        ? store.defaultSpawnModel
        : store.selectedAgent?.model ?? DEFAULT_MODEL;
      effort = fresh
        ? store.defaultSpawnEffort
        : store.selectedAgent?.effort ?? DEFAULT_EFFORT;
    });
  });

  // The blank page exists to be typed into, so put the cursor there.
  $effect(() => {
    if (drafting) textarea?.focus();
  });

  async function send() {
    if (!prompt.trim() || inFlight) return;
    // Mid-Turn, the same gesture lines the message up instead: one `claude` per
    // worktree, so it goes out as its own Turn once this one ends.
    if (working) {
      if (queueing && store.enqueue(prompt, model, effort)) prompt = "";
      return;
    }
    // Keep the text on failure either way, so the user can retry rather
    // than retype.
    const sent = drafting
      ? await store.spawn(prompt, model, effort)
      : await store.resume(prompt, model, effort);
    if (sent) prompt = "";
  }

  function onKeydown(e: KeyboardEvent) {
    // Esc on an untouched blank page walks back out of it.
    if (e.key === "Escape" && drafting && !prompt.trim() && !inFlight) {
      e.preventDefault();
      store.cancelDraft();
      return;
    }
    if (e.key !== "Enter" || e.shiftKey || e.altKey) return;
    e.preventDefault();
    send();
  }
</script>

{#if agent || drafting}
  <div class="composer" class:busy={inFlight}>
    {#if agent}
      <!-- Directly above the box it was typed into, so what's waiting sits
           where the user left it — including on an agent that can no longer be
           continued, where clearing it is the only thing left to do. -->
      <AgentQueue agentId={agent.id} running={working} />
    {/if}
    {#if drafting || store.canContinue || working}
      <div class="box">
        <textarea
          bind:this={textarea}
          bind:value={prompt}
          onkeydown={onKeydown}
          rows="1"
          class:two-up={queueing}
          disabled={inFlight}
          placeholder={drafting
            ? "What should the agent do?"
            : queueing
              ? "Working… type to queue a message"
              : working
                ? "Working… stop it to change course"
                : "Reply to this agent…"}
        ></textarea>
        <div class="slot">
          {#if working}
            <!-- While the agent runs, interrupting it lives in the same corner
                 as sending: one place to look, whichever the moment needs. -->
            <button
              class="send stop"
              onclick={() => store.stopAgent(agent!.id)}
              aria-label="Stop this agent"
              title="Stop this agent"
            >
              <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
                <rect x="5" y="5" width="6" height="6" rx="1.2" fill="currentColor" />
              </svg>
            </button>
          {/if}
          {#if queueing}
            <!-- Outlined rather than filled: the same gesture as send, but the
                 message waits for the agent instead of reaching it now. -->
            <button
              class="send queue"
              onclick={send}
              disabled={inFlight || !prompt.trim()}
              aria-label="Queue this message"
              title="Queue this message for when the agent finishes (Enter)"
            >
              <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
                <path
                  d="M8 10.5V3.5M8 3.5L4.5 7M8 3.5L11.5 7M3.5 13.5h9"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="1.8"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                />
              </svg>
            </button>
          {:else if !working}
            <button
              class="send"
              onclick={send}
              disabled={inFlight || !prompt.trim()}
              aria-label={drafting ? "Spawn this agent" : "Send"}
              title={drafting ? "Spawn this agent (Enter)" : "Send (Enter)"}
            >
              <!-- Arrow up: the send affordance every chat box uses, so it needs
                   no label to read as "send". -->
              <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
                <path
                  d="M8 13V3.5M8 3.5L3.5 8M8 3.5L12.5 8"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="1.8"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                />
              </svg>
            </button>
          {/if}
        </div>
      </div>
      <div class="below">
        <span class="hint">
          {#if store.spawning}
            Spawning…
          {:else if store.sending}
            Sending…
          {/if}
        </span>
        <ModelPicker
          bind:value={model}
          bind:effort
          disabled={inFlight}
          compact
          label={drafting ? "Model for the new agent" : "Model for this prompt"}
        />
      </div>
    {:else if agent}
      <p class="closed">
        This conversation can't be continued
        {#if !agent.session_id}
          — it ran before follow-ups were possible.
        {:else}
          — its worktree is gone.
        {/if}
        Spawn a new agent to keep going.
      </p>
    {/if}
  </div>
{/if}

<style>
  /*
   * No rule above it: the transcript and the box it's answered in are one
   * surface, and a divider there reads as two panes bolted together.
   */
  .composer {
    flex: none;
    background: var(--surface);
    padding: 0.35rem var(--pad-x) 0.7rem;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  /* The bordered rectangle the user thinks of as "the input" — the textarea
     and the send button both live inside it and share its focus ring. */
  .box {
    position: relative;
    display: flex;
    border: 1px solid var(--border);
    border-radius: 14px;
    background: var(--panel-bg);
    transition: border-color 0.12s ease;
  }

  .box:focus-within {
    border-color: var(--accent);
  }

  textarea {
    flex: 1;
    min-width: 0;
    resize: none;
    max-height: 30vh;
    overflow-y: auto;
    /* Right padding clears the send button so text never runs under it. */
    padding: 0.7rem 3rem 0.7rem 0.85rem;
    border: none;
    background: none;
    color: var(--fg);
    font: inherit;
    font-size: 0.9rem;
    line-height: 1.5;
  }

  /* Clears both buttons when Stop and Queue share the corner. */
  textarea.two-up {
    padding-right: 5.1rem;
  }

  textarea:focus {
    outline: none;
  }

  textarea:disabled {
    color: var(--fg-muted);
    cursor: not-allowed;
  }

  /* Pinned to the bottom-right of the box, so the buttons stay put as the text
     grows — and Stop keeps its corner when a queue button appears beside it. */
  .slot {
    position: absolute;
    right: 0.45rem;
    bottom: 0.45rem;
    display: flex;
    align-items: center;
    gap: 0.3rem;
  }

  .send {
    width: 1.85rem;
    height: 1.85rem;
    display: flex;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: 999px;
    background: var(--accent);
    color: var(--surface);
    cursor: pointer;
    transition:
      opacity 0.12s ease,
      filter 0.12s ease;
  }

  .send:hover:not(:disabled) {
    filter: brightness(1.08);
  }

  .send:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .send:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }

  /* Same slot, different job: red reads as "interrupt" without a label. */
  .send.stop {
    background: #ef4444;
    color: #fff;
  }

  /* Outlined: sending's sibling, held back a step. */
  .send.queue {
    background: none;
    border: 1.5px solid var(--accent);
    color: var(--accent);
  }

  .send.queue:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    filter: none;
  }

  .below {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
    padding: 0 0.15rem;
  }

  .hint {
    font-size: 0.7rem;
    color: var(--fg-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .closed {
    margin: 0;
    font-size: 0.82rem;
    color: var(--fg-muted);
    font-style: italic;
  }

  /* On a narrow pane the hint is the first thing worth dropping — the model
     picker stays on the right either way. */
  @media (max-width: 560px) {
    .hint {
      display: none;
    }

    .below {
      justify-content: flex-end;
    }
  }
</style>
