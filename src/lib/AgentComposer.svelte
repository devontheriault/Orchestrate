<script lang="ts">
  import { untrack } from "svelte";
  import { store } from "./store.svelte";
  import { DEFAULT_EFFORT, DEFAULT_MODE, DEFAULT_MODEL } from "./api";
  import ModelPicker from "./ModelPicker.svelte";
  import ModePicker from "./ModePicker.svelte";
  import AgentQueue from "./AgentQueue.svelte";
  import TurnStats from "./TurnStats.svelte";

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
   * Whether Enter would queue what's typed rather than send it. Needs a session
   * to resume into later: on an agent that can't be continued at all there is
   * nothing to line a message up behind.
   */
  const queueing = $derived(working && !!agent?.session_id);

  let prompt = $state("");
  /** The picker's value; DEFAULT_MODEL passes no --model. */
  let model = $state(DEFAULT_MODEL);
  /** The effort chosen beside it; DEFAULT_EFFORT passes no --effort. */
  let effort = $state(DEFAULT_EFFORT);
  /** What this turn may do without asking: YOLO, or plan and write nothing. */
  let mode = $state(DEFAULT_MODE);
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
      mode = fresh
        ? store.defaultSpawnMode
        : store.selectedAgent?.permission_mode ?? DEFAULT_MODE;
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
      if (queueing && store.enqueue(prompt, model, effort, mode)) prompt = "";
      return;
    }
    // Keep the text on failure either way, so the user can retry rather
    // than retype.
    const sent = drafting
      ? await store.spawn(prompt, model, effort, mode)
      : await store.resume(prompt, model, effort, mode);
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
    {#if working}
      <!-- Last thing before the box: while the agent works, how long it's been
           and how much it's written sit right where the eye already is. -->
      <TurnStats />
    {/if}
    {#if drafting || store.canContinue || working}
      <div class="box">
        <textarea
          bind:this={textarea}
          bind:value={prompt}
          onkeydown={onKeydown}
          rows="1"
          disabled={inFlight}
          placeholder={drafting
            ? "What should the agent do?"
            : queueing
              ? "Working… press Enter to queue a message"
              : working
                ? "Working… stop it to change course"
                : "Reply to this agent…"}
        ></textarea>
        <div class="slot">
          {#if working}
            <!-- Stop takes the send button's place rather than sitting beside
                 it: one corner, one button, whichever the moment calls for. -->
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
          {:else}
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
        <div class="picks">
          <ModelPicker
            bind:value={model}
            bind:effort
            disabled={inFlight}
            compact
            label={drafting ? "Model for the new agent" : "Model for this prompt"}
          />
          <!-- Right of the model: the same decision, one step further out —
               which model runs the turn, and what it's allowed to do. -->
          <ModePicker
            bind:value={mode}
            disabled={inFlight}
            compact
            label={drafting ? "Mode for the new agent" : "Mode for this prompt"}
          />
        </div>
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
    gap: var(--space-3);
  }

  /* The bordered rectangle the user thinks of as "the input" — the textarea
     and the send button both live inside it and share its focus ring. */
  .box {
    position: relative;
    display: flex;
    border: 1px solid var(--border);
    border-radius: var(--radius-xl);
    background: var(--panel-bg);
    transition: border-color var(--transition-fast);
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
    font-size: var(--text-lg);
    line-height: var(--leading-normal);
  }

  textarea:focus {
    outline: none;
  }

  textarea:disabled {
    color: var(--fg-muted);
    cursor: not-allowed;
  }

  /* Pinned to the bottom-right of the box, so whichever button the moment
     calls for stays put as the text grows. */
  .slot {
    position: absolute;
    right: 0.45rem;
    bottom: 0.45rem;
    display: flex;
    align-items: center;
  }

  .send {
    width: 1.85rem;
    height: 1.85rem;
    display: flex;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: var(--radius-pill);
    background: var(--accent);
    color: var(--surface);
    cursor: pointer;
    transition:
      opacity var(--transition-fast),
      filter var(--transition-fast);
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
    background: var(--danger);
    color: var(--on-danger);
  }

  .below {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-5);
    padding: 0 0.15rem;
  }

  /* The two pickers read as one group on the right — model, then what it may
     do — rather than two controls that happen to share a row. */
  .picks {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
  }

  .hint {
    font-size: var(--text-2xs);
    color: var(--fg-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .closed {
    margin: 0;
    font-size: var(--text-md);
    color: var(--fg-muted);
    font-style: italic;
  }

  /* On a narrow pane the hint is the first thing worth dropping — the pickers
     stay on the right either way. */
  @media (max-width: 560px) {
    .hint {
      display: none;
    }

    .below {
      justify-content: flex-end;
    }
  }
</style>
