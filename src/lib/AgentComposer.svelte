<script lang="ts">
  import { untrack } from "svelte";
  import { store } from "./store.svelte";
  import { DEFAULT_EFFORT, DEFAULT_MODEL } from "./api";
  import ModelPicker from "./ModelPicker.svelte";

  const agent = $derived(store.selectedAgent);
  /** No agent yet: this composer is holding the opening prompt for a new one. */
  const drafting = $derived(store.drafting && !agent);
  const working = $derived(agent?.state === "running");
  const busy = $derived(working || store.sending || store.spawning);

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
    if (!prompt.trim() || busy) return;
    // Keep the text on failure either way, so the user can retry rather
    // than retype.
    const sent = drafting
      ? await store.spawn(prompt, model, effort)
      : await store.resume(prompt, model, effort);
    if (sent) prompt = "";
  }

  function onKeydown(e: KeyboardEvent) {
    // Esc on an untouched blank page walks back out of it.
    if (e.key === "Escape" && drafting && !prompt.trim() && !busy) {
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
  <div class="composer" class:busy>
    {#if drafting || store.canContinue || working}
      <div class="box">
        <textarea
          bind:this={textarea}
          bind:value={prompt}
          onkeydown={onKeydown}
          rows="1"
          disabled={busy}
          placeholder={drafting
            ? "What should the agent do?"
            : working
              ? "Working… stop it to change course"
              : "Reply to this agent…"}
        ></textarea>
        <button
          class="send"
          onclick={send}
          disabled={busy || !prompt.trim()}
          aria-label={drafting ? "Spawn this agent" : "Send"}
          title={drafting
            ? "Spawn this agent (Enter)"
            : working
              ? "Wait for the agent to finish"
              : "Send (Enter)"}
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
      </div>
      <div class="below">
        <span class="hint">
          {#if store.spawning}
            Spawning…
          {:else if store.sending}
            Sending…
          {:else if working}
            Stop the agent to send a new prompt
          {/if}
        </span>
        <ModelPicker
          bind:value={model}
          bind:effort
          disabled={busy}
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

  textarea:focus {
    outline: none;
  }

  textarea:disabled {
    color: var(--fg-muted);
    cursor: not-allowed;
  }

  /* Pinned to the bottom-right of the box, so it stays put as the text grows. */
  .send {
    position: absolute;
    right: 0.45rem;
    bottom: 0.45rem;
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
