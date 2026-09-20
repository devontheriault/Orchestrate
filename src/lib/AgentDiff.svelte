<script lang="ts">
  import { store } from "./store.svelte";

  // Suggested commit message: the agent's own prompt, as a subject line.
  const suggested = $derived.by(() => {
    const prompt = store.selectedAgent?.task.prompt ?? "";
    const subject = prompt.split("\n")[0].replace(/\s+/g, " ").trim();
    return subject.length > 72 ? subject.slice(0, 72).trimEnd() : subject;
  });

  let message = $state("");
  let messageEdited = $state(false);

  // Follow the suggestion until the user types something of their own.
  $effect(() => {
    const s = suggested;
    if (!messageEdited) message = s;
  });

  // A new selection resets the editing state along with the message.
  $effect(() => {
    store.selectedAgentId;
    messageEdited = false;
  });

  const running = $derived(store.selectedAgent?.state === "running");

  const totals = $derived.by(() => {
    const files = store.diff?.files ?? [];
    return files.reduce(
      (acc, f) => ({
        insertions: acc.insertions + (f.insertions ?? 0),
        deletions: acc.deletions + (f.deletions ?? 0),
      }),
      { insertions: 0, deletions: 0 },
    );
  });

  /** Cap on rendered patch lines, so a huge diff can't stall the webview. */
  const MAX_LINES = 6000;

  type PatchLine = { text: string; kind: "add" | "del" | "hunk" | "ctx" };
  type PatchFile = { path: string; lines: PatchLine[] };

  // Split the raw patch into per-file sections, dropping the git metadata
  // lines (index/---/+++/mode) that the file header already conveys.
  const patchFiles = $derived.by((): { files: PatchFile[]; clipped: boolean } => {
    const patch = store.diff?.patch ?? "";
    if (!patch) return { files: [], clipped: false };

    const files: PatchFile[] = [];
    let budget = MAX_LINES;
    let clipped = false;

    for (const raw of patch.split("\n")) {
      if (raw.startsWith("diff --git ")) {
        const m = raw.match(/ b\/(.*)$/);
        files.push({ path: m ? m[1] : raw.slice("diff --git ".length), lines: [] });
        continue;
      }
      const current = files[files.length - 1];
      if (!current) continue;
      if (
        /^(index |--- |\+\+\+ |old mode |new mode |new file mode |deleted file mode |similarity index |rename (from|to) )/.test(
          raw,
        )
      ) {
        continue;
      }
      if (budget <= 0) {
        clipped = true;
        continue;
      }
      budget--;
      const kind: PatchLine["kind"] = raw.startsWith("@@")
        ? "hunk"
        : raw.startsWith("+")
          ? "add"
          : raw.startsWith("-")
            ? "del"
            : "ctx";
      current.lines.push({ text: raw, kind });
    }
    return { files, clipped };
  });

  async function doCommit() {
    if (await store.commit(message)) messageEdited = false;
  }
</script>

<div class="diff-pane">
  {#if store.diffLoading && !store.diff}
    <div class="hint">Reading the worktree…</div>
  {:else if store.diffError && !store.diff}
    <div class="diff-error">{store.diffError}</div>
  {:else if store.diff}
    {@const diff = store.diff}

    <div class="summary">
      <div class="counts">
        <span class="n">{diff.files.length}</span>
        file{diff.files.length === 1 ? "" : "s"}
        <span class="ins">+{totals.insertions}</span>
        <span class="del">−{totals.deletions}</span>
      </div>
      <div class="summary-right">
        <span class="mono base">base {diff.base.slice(0, 7)}</span>
        <button onclick={() => store.loadDiff()} disabled={store.diffLoading}>
          {store.diffLoading ? "Refreshing…" : "Refresh"}
        </button>
      </div>
    </div>

    {#if store.diffError}
      <div class="diff-error">{store.diffError}</div>
    {/if}

    {#if diff.files.length === 0 && diff.commits.length === 0}
      <div class="hint">This agent changed nothing in its worktree.</div>
    {/if}

    {#if diff.commits.length > 0}
      <div class="commits">
        <div class="section-label">
          {diff.commits.length} commit{diff.commits.length === 1 ? "" : "s"} on
          <code>{store.selectedAgent?.branch}</code>
        </div>
        {#each diff.commits as c (c.sha)}
          <div class="commit">
            <code class="sha">{c.sha}</code>
            <span class="subject">{c.subject}</span>
          </div>
        {/each}
      </div>
    {/if}

    {#if diff.uncommitted}
      <div class="commit-box">
        <div class="commit-box-head">
          <span class="warn-dot"></span>
          <strong>Uncommitted work</strong>
          <span class="sub">A reap now would discard it.</span>
        </div>
        <textarea
          bind:value={message}
          oninput={() => (messageEdited = true)}
          rows="2"
          placeholder="Commit message"
          disabled={running || store.committing}
        ></textarea>
        <div class="commit-box-foot">
          {#if running}
            <span class="sub">Stop the agent before committing — it is still writing.</span>
          {:else}
            <span class="sub">Stages everything in the worktree onto the agent's branch.</span>
          {/if}
          <button
            class="primary"
            disabled={running || store.committing || message.trim().length === 0}
            onclick={doCommit}
          >
            {store.committing ? "Committing…" : "Commit all changes"}
          </button>
        </div>
      </div>
    {/if}

    {#if diff.files.length > 0}
      <div class="files">
        {#each diff.files as f (f.path)}
          <div class="file-row">
            <span class={`badge badge-${f.status.toLowerCase()}`}>{f.status}</span>
            <span class="path mono">{f.path}</span>
            {#if f.insertions === null}
              <span class="binary">binary</span>
            {:else}
              <span class="ins">+{f.insertions}</span>
              <span class="del">−{f.deletions}</span>
            {/if}
          </div>
        {/each}
      </div>
    {/if}

    {#each patchFiles.files as pf (pf.path)}
      <details class="patch-file" open>
        <summary class="mono">{pf.path}</summary>
        <div class="patch">
          {#each pf.lines as line, i (i)}
            <div class={`line ${line.kind}`}>{line.text || " "}</div>
          {/each}
        </div>
      </details>
    {/each}

    {#if patchFiles.clipped || diff.truncated}
      <div class="clipped">
        Patch shown in part only. Open
        <code>{store.selectedAgent?.worktree_path}</code>
        to read all of it.
      </div>
    {/if}
  {/if}
</div>

<style>
  .diff-pane {
    flex: 1;
    overflow-y: auto;
    padding: 1rem 1.25rem 2rem;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  .hint {
    color: var(--fg-muted);
    font-style: italic;
    text-align: center;
    padding: 3rem 1rem;
    font-size: 0.9rem;
  }

  .mono {
    font-family: ui-monospace, monospace;
  }

  code {
    font-family: ui-monospace, monospace;
    background: var(--code-bg);
    padding: 0.05em 0.35em;
    border-radius: 3px;
  }

  .summary {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    font-size: 0.85rem;
    color: var(--fg-muted);
  }

  .summary .n {
    color: var(--fg);
    font-weight: 600;
  }

  .summary-right {
    display: flex;
    align-items: center;
    gap: 0.6rem;
  }

  .base {
    font-size: 0.78rem;
  }

  .summary button {
    border-radius: 5px;
    border: 1px solid var(--border);
    background: var(--surface);
    color: var(--fg);
    padding: 0.28rem 0.7rem;
    font-size: 0.8rem;
    cursor: pointer;
  }

  .summary button:hover:not(:disabled) {
    border-color: var(--accent);
    color: var(--accent);
  }

  .summary button:disabled {
    opacity: 0.6;
    cursor: default;
  }

  .ins {
    color: #16a34a;
    font-family: ui-monospace, monospace;
  }

  .del {
    color: #dc2626;
    font-family: ui-monospace, monospace;
  }

  .section-label {
    font-size: 0.78rem;
    color: var(--fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin-bottom: 0.4rem;
  }

  .section-label code {
    text-transform: none;
    letter-spacing: 0;
  }

  .commits {
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--panel-bg);
    padding: 0.6rem 0.8rem;
  }

  .commit {
    display: flex;
    gap: 0.6rem;
    align-items: baseline;
    font-size: 0.85rem;
    padding: 0.12rem 0;
  }

  .commit .sha {
    color: var(--accent);
    background: transparent;
    padding: 0;
  }

  .commit .subject {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .commit-box {
    border: 1px solid rgba(245, 158, 11, 0.4);
    background: rgba(245, 158, 11, 0.08);
    border-radius: 6px;
    padding: 0.7rem 0.85rem;
    display: flex;
    flex-direction: column;
    gap: 0.55rem;
  }

  .commit-box-head {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.87rem;
  }

  .warn-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: #f59e0b;
    flex: none;
  }

  .sub {
    color: var(--fg-muted);
    font-size: 0.8rem;
  }

  .commit-box textarea {
    width: 100%;
    resize: vertical;
    font-family: inherit;
    font-size: 0.87rem;
    line-height: 1.5;
    color: var(--fg);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 5px;
    padding: 0.45rem 0.6rem;
  }

  .commit-box textarea:focus {
    outline: none;
    border-color: var(--accent);
  }

  .commit-box textarea:disabled {
    opacity: 0.6;
  }

  .commit-box-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
  }

  .commit-box-foot button.primary {
    background: #f59e0b;
    border: 1px solid #f59e0b;
    color: #1a1206;
    font-weight: 500;
    border-radius: 5px;
    padding: 0.35rem 0.85rem;
    font-size: 0.83rem;
    cursor: pointer;
    flex: none;
  }

  .commit-box-foot button.primary:hover:not(:disabled) {
    filter: brightness(1.08);
  }

  .commit-box-foot button.primary:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .files {
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
  }

  .file-row {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    padding: 0.35rem 0.7rem;
    font-size: 0.83rem;
    border-bottom: 1px solid var(--border);
  }

  .file-row:last-child {
    border-bottom: none;
  }

  .file-row .path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .badge {
    width: 1.15rem;
    text-align: center;
    font-family: ui-monospace, monospace;
    font-size: 0.72rem;
    font-weight: 700;
    border-radius: 3px;
    background: var(--code-bg);
    color: var(--fg-muted);
    flex: none;
  }

  .badge-a { color: #16a34a; }
  .badge-m { color: #d97706; }
  .badge-d { color: #dc2626; }

  .binary {
    color: var(--fg-muted);
    font-size: 0.78rem;
    font-style: italic;
  }

  .patch-file {
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
  }

  .patch-file summary {
    cursor: pointer;
    padding: 0.4rem 0.7rem;
    background: var(--panel-bg);
    font-size: 0.82rem;
    color: var(--fg-muted);
  }

  .patch {
    overflow-x: auto;
    font-family: ui-monospace, monospace;
    font-size: 0.78rem;
    line-height: 1.55;
  }

  .line {
    padding: 0 0.7rem;
    white-space: pre;
    min-width: max-content;
  }

  .line.add {
    background: rgba(22, 163, 74, 0.12);
    color: #15803d;
  }

  .line.del {
    background: rgba(220, 38, 38, 0.1);
    color: #b91c1c;
  }

  .line.hunk {
    background: var(--code-bg);
    color: var(--fg-muted);
  }

  .diff-error,
  .clipped {
    border-radius: 6px;
    padding: 0.55rem 0.85rem;
    font-size: 0.84rem;
  }

  .diff-error {
    background: rgba(239, 68, 68, 0.08);
    border: 1px solid rgba(239, 68, 68, 0.3);
    color: var(--fg);
    font-family: ui-monospace, monospace;
    white-space: pre-wrap;
  }

  .clipped {
    background: var(--panel-bg);
    border: 1px solid var(--border);
    color: var(--fg-muted);
  }

  @media (prefers-color-scheme: dark) {
    .line.add { color: #86efac; }
    .line.del { color: #fca5a5; }
  }
</style>
