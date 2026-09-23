<script lang="ts">
  /**
   * Asked once, when a Project is added. Agents run `claude --print`, which
   * skips Claude Code's own workspace trust prompt, so registering a Project
   * is the only point at which anyone decides to trust its contents.
   *
   * A folder that isn't a Git repository with a commit yet (`needsSetup`) is
   * set up on confirm, so the same answer covers that too.
   */
  let {
    path,
    needsSetup,
    onconfirm,
    oncancel,
  }: {
    path: string;
    needsSetup: boolean;
    onconfirm: () => Promise<void>;
    oncancel: () => void;
  } = $props();

  let confirmEl: HTMLButtonElement | undefined = $state();
  $effect(() => confirmEl?.focus());

  /** Confirmed and still adding; nothing to cancel by then. */
  let busy = $state(false);

  async function confirm() {
    busy = true;
    await onconfirm();
  }

  function cancel() {
    if (!busy) oncancel();
  }

  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") cancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });
</script>

<!-- Closes on a click outside the dialog, not on one that lands inside it. -->
<div
  class="backdrop"
  role="presentation"
  onclick={(e) => {
    if (e.target === e.currentTarget) cancel();
  }}
>
  <div
    class="dialog"
    role="alertdialog"
    aria-modal="true"
    aria-labelledby="trust-title"
    aria-describedby="trust-body"
  >
    <h2 id="trust-title">Trust this project?</h2>
    <code class="path">{path}</code>

    <div id="trust-body" class="body">
      <p>
        Agents run Claude Code here without asking whether you trust the folder first,
        so the project's own Claude setup takes effect right away:
      </p>
      <ul>
        <li>its <code>CLAUDE.md</code> and skills shape what every agent does</li>
        <li>hooks in <code>.claude/settings.json</code> run commands on this machine</li>
        <li>agents start in YOLO mode and act without asking for permission</li>
      </ul>
      <p class="advice">Only add code you wrote or trust.</p>
    </div>

    {#if needsSetup}
      <div class="setup">
        <p>
          This folder isn't tracked yet. It will be set up so agents can work on their own
          copies without touching your files.
        </p>
        <details>
          <summary>Details</summary>
          <p>
            Initializes Git, adds a <code>.gitignore</code> (for <code>.env</code>,
            <code>node_modules</code>, build folders…) if there isn't one, and saves a first
            snapshot of your files.
          </p>
        </details>
      </div>
    {/if}

    <footer>
      <button class="btn" onclick={cancel} disabled={busy}>Cancel</button>
      <button
        bind:this={confirmEl}
        class="btn btn-primary"
        onclick={confirm}
        disabled={busy}
      >
        {#if busy}
          {needsSetup ? "Setting up…" : "Adding…"}
        {:else}
          {needsSetup ? "Set up and add" : "Trust and add"}
        {/if}
      </button>
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: var(--scrim);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: var(--space-6);
    z-index: var(--z-overlay);
    backdrop-filter: blur(2px);
  }

  .dialog {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    width: min(30rem, 100%);
    max-height: 100%;
    overflow-y: auto;
    box-shadow: var(--shadow-modal);
    padding: 1.1rem 1.25rem 1rem;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  h2 {
    font-size: var(--text-2xl);
    font-weight: var(--weight-semibold);
    margin: 0;
  }

  .path {
    display: block;
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    color: var(--fg-muted);
    background: var(--code-bg);
    border-radius: var(--radius-sm);
    padding: var(--space-2) var(--space-3);
    /* Wrapped rather than truncated: the whole path is what's being trusted. */
    overflow-wrap: anywhere;
  }

  .body {
    font-size: var(--text-md);
    line-height: var(--leading-normal);
  }

  .body p {
    margin: 0;
  }

  .body ul {
    margin: var(--space-3) 0;
    padding-left: 1.2rem;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .body code {
    font-family: var(--font-mono);
    font-size: 0.92em;
  }

  .advice {
    color: var(--fg-muted);
  }

  .setup {
    font-size: var(--text-md);
    line-height: var(--leading-normal);
    background: var(--panel-bg);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    padding: var(--space-4) var(--space-5);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .setup p {
    margin: 0;
  }

  .setup summary {
    cursor: pointer;
    color: var(--fg-muted);
    width: fit-content;
  }

  .setup details p {
    margin-top: var(--space-2);
    color: var(--fg-muted);
  }

  .setup code {
    font-family: var(--font-mono);
    font-size: 0.92em;
  }

  footer {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-3);
  }
</style>
