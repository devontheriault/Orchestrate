<script lang="ts">
  import { untrack } from "svelte";
  import { store } from "./store.svelte";
  import { DEFAULT_MODEL } from "./api";
  import ModelPicker from "./ModelPicker.svelte";

  const agent = $derived(store.selectedAgent);
  /** No agent yet: this composer is holding the opening prompt for a new one. */
  const drafting = $derived(store.drafting && !agent);
  const working = $derived(agent?.state === "running");
  const busy = $derived(working || store.sending || store.spawning);

  let prompt = $state("");
  /** The picker's value; DEFAULT_MODEL passes no --model. */
  let model = $state(DEFAULT_MODEL);
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
    model = untrack(() =>
      store.drafting && !store.selectedAgent
        ? store.defaultSpawnModel
        : store.selectedAgent?.model ?? DEFAULT_MODEL,
    );
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
      ? await store.spawn(prompt, model)
      : await store.resume(prompt, model);
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
      <div class="side">
        <div class="controls">
          <ModelPicker
            bind:value={model}
            disabled={busy}
            compact
            label={drafting ? "Model for the new agent" : "Model for this prompt"}
          />
          <button
            class="send"
            onclick={send}
            disabled={busy || !prompt.trim()}
            title={drafting
              ? "Spawn this agent (Enter)"
              : working
                ? "Wait for the agent to finish"
                : "Send (Enter)"}
          >
            {#if drafting}
              {store.spawning ? "Spawning…" : "Spawn"}
            {:else}
              {store.sending ? "Sending…" : "Send"}
            {/if}
          </button>
        </div>
        <span class="hint">
          {#if drafting}
            Enter to spawn · Esc to cancel
          {:else if working}
            Stop the agent to send a new prompt
          {:else}
            Enter to send · Shift-Enter for a new line
          {/if}
        </span>
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
  .composer {
    flex: none;
    border-top: 1px solid var(--border);
    background: var(--surface);
    padding: 0.6rem var(--pad-x) 0.7rem;
    display: flex;
    align-items: flex-end;
    gap: 0.6rem;
  }

  textarea {
    flex: 1;
    min-width: 0;
    resize: none;
    max-height: 30vh;
    overflow-y: auto;
    padding: 0.5rem 0.65rem;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--panel-bg);
    color: var(--fg);
    font: inherit;
    font-size: 0.9rem;
    line-height: 1.5;
  }

  textarea:focus {
    outline: none;
    border-color: var(--accent);
  }

  textarea:disabled {
    color: var(--fg-muted);
    cursor: not-allowed;
  }

  .side {
    flex: none;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 0.25rem;
  }

  .controls {
    display: flex;
    align-items: center;
    gap: 0.35rem;
  }

  .send {
    border: 1px solid var(--accent);
    background: var(--accent);
    color: var(--surface);
    border-radius: 6px;
    padding: 0.42rem 1rem;
    font-family: inherit;
    font-size: 0.85rem;
    font-weight: 500;
    cursor: pointer;
  }

  .send:hover:not(:disabled) {
    filter: brightness(1.08);
  }

  .send:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .hint {
    font-size: 0.7rem;
    color: var(--fg-muted);
    white-space: nowrap;
  }

  .closed {
    margin: 0;
    font-size: 0.82rem;
    color: var(--fg-muted);
    font-style: italic;
  }

  /* On a narrow pane the hint is the first thing worth dropping. */
  @media (max-width: 560px) {
    .hint {
      display: none;
    }
  }
</style>
