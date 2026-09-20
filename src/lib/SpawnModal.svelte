<script lang="ts">
  import { store } from "./store.svelte";
  import { DEFAULT_MODEL } from "./api";
  import ModelPicker from "./ModelPicker.svelte";

  let { onClose }: { onClose: () => void } = $props();

  let prompt = $state("");
  // Opens on the user's last pick — most people spawn on the same model daily.
  let model = $state(store.preferredModel ?? DEFAULT_MODEL);
  let submitting = $state(false);
  let textarea: HTMLTextAreaElement | undefined = $state();

  $effect(() => {
    textarea?.focus();
  });

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (!prompt.trim() || submitting) return;
    submitting = true;
    await store.spawn(prompt, model || null);
    submitting = false;
    onClose();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      onClose();
      return;
    }
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      const form = (e.target as HTMLElement).closest("form");
      form?.requestSubmit();
    }
  }

  $effect(() => {
    window.addEventListener("keydown", onKeydown);
    return () => window.removeEventListener("keydown", onKeydown);
  });

  const projectName = $derived(
    store.projects.find((p) => p.id === store.selectedProjectId)?.name ?? "",
  );
</script>

<div
  class="backdrop"
  role="presentation"
  onclick={onClose}
>
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="spawn-title"
    tabindex="-1"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.stopPropagation()}
  >
  <form onsubmit={submit}>
    <header>
      <h2 id="spawn-title">Spawn agent</h2>
      <span class="target">in {projectName}</span>
    </header>

    <textarea
      bind:this={textarea}
      bind:value={prompt}
      placeholder="What should the agent do?"
      rows="8"
      disabled={submitting}
    ></textarea>

    <div class="model-row">
      <span class="label">Model</span>
      <ModelPicker bind:value={model} disabled={submitting} />
      {#if store.modelsLoading}
        <span class="note">loading models…</span>
      {:else if store.modelsError}
        <span class="note" title={store.modelsError}>
          couldn't load your models — Default still works
        </span>
      {/if}
    </div>

    <footer>
      <span class="hint">⌘/Ctrl-Enter to spawn · Esc to cancel</span>
      <div class="buttons">
        <button type="button" onclick={onClose} disabled={submitting}>Cancel</button>
        <button
          type="submit"
          class="primary"
          disabled={submitting || !prompt.trim()}>
          {submitting ? "Spawning…" : "Spawn"}
        </button>
      </div>
    </footer>
  </form>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.4);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 1rem;
    z-index: 100;
    backdrop-filter: blur(2px);
  }

  .dialog {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 1.25rem 1.5rem 1rem;
    /* Grows with the window, but never past a comfortable reading width. */
    width: min(40rem, 100%);
    max-height: 100%;
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.25);
    display: flex;
    flex-direction: column;
    gap: 0.85rem;
    overflow: hidden;
  }

  .dialog form {
    display: flex;
    flex: 1 1 auto;
    flex-direction: column;
    gap: 0.85rem;
    min-height: 0;
  }

  header {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.25rem 1rem;
  }

  h2 {
    font-size: 1.05rem;
    margin: 0;
    font-weight: 600;
  }

  .target {
    font-size: 0.8rem;
    color: var(--fg-muted);
    overflow-wrap: anywhere;
  }

  textarea {
    width: 100%;
    box-sizing: border-box;
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.7rem 0.85rem;
    font-family: inherit;
    font-size: 0.93rem;
    line-height: 1.5;
    resize: vertical;
    /* Takes the height the window can spare, shrinking on a short one. */
    flex: 1 1 auto;
    height: clamp(5rem, 34vh, 16rem);
    min-height: 3rem;
    background: var(--panel-bg);
    color: var(--fg);
  }

  textarea:focus {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 3px rgba(59, 130, 246, 0.15);
  }

  .model-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem 0.6rem;
    min-width: 0;
  }

  .model-row .label {
    font-size: 0.8rem;
    color: var(--fg-muted);
  }

  .note {
    font-size: 0.73rem;
    color: var(--fg-muted);
    overflow-wrap: anywhere;
  }

  footer {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem 1rem;
  }

  .hint {
    font-size: 0.75rem;
    color: var(--fg-muted);
  }

  .buttons {
    display: flex;
    gap: 0.5rem;
    margin-left: auto;
  }

  .buttons button {
    padding: 0.5rem 1rem;
    border-radius: 6px;
    border: 1px solid var(--border);
    background: var(--surface);
    font-family: inherit;
    font-size: 0.88rem;
    color: var(--fg);
    cursor: pointer;
  }

  .buttons button:hover:not(:disabled) {
    border-color: var(--accent);
  }

  .buttons .primary {
    background: var(--accent);
    color: white;
    border-color: var(--accent);
  }

  .buttons .primary:hover:not(:disabled) {
    filter: brightness(1.1);
  }

  .buttons button:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
