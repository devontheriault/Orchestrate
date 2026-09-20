<script lang="ts">
  import { store } from "./store.svelte";

  const agent = $derived(store.selectedAgent);
  const working = $derived(agent?.state === "running");
  const busy = $derived(working || store.sending);

  let prompt = $state("");
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

  // A different agent is a different conversation: don't carry a draft over.
  $effect(() => {
    store.selectedAgentId;
    prompt = "";
  });

  async function send() {
    if (!prompt.trim() || busy) return;
    const sent = await store.resume(prompt);
    // Keep the text on failure so the user can retry rather than retype.
    if (sent) prompt = "";
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key !== "Enter" || e.shiftKey || e.altKey) return;
    e.preventDefault();
    send();
  }
</script>

{#if agent}
  <div class="composer" class:busy>
    {#if store.canContinue || working}
      <textarea
        bind:this={textarea}
        bind:value={prompt}
        onkeydown={onKeydown}
        rows="1"
        disabled={busy}
        placeholder={working
          ? "Working… stop it to change course"
          : "Reply to this agent…"}
      ></textarea>
      <div class="side">
        <button
          class="send"
          onclick={send}
          disabled={busy || !prompt.trim()}
          title={working ? "Wait for the agent to finish" : "Send (Enter)"}
        >
          {store.sending ? "Sending…" : "Send"}
        </button>
        <span class="hint">
          {#if working}
            turn {agent.turns}
          {:else}
            Enter to send · Shift-Enter for a new line
          {/if}
        </span>
      </div>
    {:else}
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
