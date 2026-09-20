<script lang="ts">
  import { store } from "./store.svelte";
  import { MODEL_CHOICES } from "./api";

  let { onClose }: { onClose: () => void } = $props();

  let prompt = $state("");
  // Opens on the user's last pick — most people spawn on the same model daily.
  let model = $state<string | null>(store.preferredModel);
  let submitting = $state(false);
  let textarea: HTMLTextAreaElement | undefined = $state();

  $effect(() => {
    textarea?.focus();
  });

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (!prompt.trim() || submitting) return;
    submitting = true;
    await store.spawn(prompt, model);
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

    <fieldset class="models" disabled={submitting}>
      <legend>Model</legend>
      {#each MODEL_CHOICES as choice (choice.label)}
        <label class="model" class:picked={model === choice.value}>
          <input
            type="radio"
            name="model"
            value={choice.value ?? ""}
            checked={model === choice.value}
            onchange={() => (model = choice.value)}
          />
          <span class="name">{choice.label}</span>
          <span class="blurb">{choice.blurb}</span>
        </label>
      {/each}
    </fieldset>

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

  .models {
    border: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
    min-width: 0;
  }

  .models legend {
    /* The choices label themselves; the legend is for screen readers. */
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }

  .model {
    flex: 1 1 8rem;
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    padding: 0.45rem 0.6rem;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--panel-bg);
    cursor: pointer;
    min-width: 0;
  }

  .model:hover {
    border-color: var(--accent);
  }

  .model.picked {
    border-color: var(--accent);
    box-shadow: inset 0 0 0 1px var(--accent);
  }

  /* The card is the control; the radio only carries the semantics. It stays in
     the flow (zero-sized) so arrow-key focus lands here, not at the corner of
     the screen, and the card shows the ring on its behalf. */
  .model input {
    appearance: none;
    width: 0;
    height: 0;
    margin: 0;
    outline: none;
  }

  .model:has(input:focus-visible) {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px rgba(59, 130, 246, 0.25);
  }

  .model .name {
    font-size: 0.85rem;
    font-weight: 500;
  }

  .model .blurb {
    font-size: 0.7rem;
    color: var(--fg-muted);
    overflow-wrap: anywhere;
  }

  .models:disabled .model {
    opacity: 0.5;
    cursor: default;
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
