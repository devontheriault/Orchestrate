<script lang="ts">
  import { store } from "$lib/state/store.svelte";
  import BranchPicker from "$lib/menus/BranchPicker.svelte";
  import CodeSpans from "$lib/code/CodeSpans.svelte";
  import { languageOf } from "$lib/code/highlight.svelte";
  import { highlightPatch, parsePatch } from "./patch";

  /** The selected agent's diff, and the Commit / Merge / Resolve taken from it. */
  const review = store.review;

  // Suggested commit message: the conversation's name, as a subject line.
  const suggested = $derived(review.suggestedMessage);

  let message = $state("");
  let messageEdited = $state(false);

  // Follow the suggestion until the user types something of their own.
  $effect(() => {
    const s = suggested;
    if (!messageEdited) message = s;
  });

  /**
   * Whether the Merge pushes its target after. Off until the user turns it on,
   * and off again for the next agent: a push is a write beyond the project.
   */
  let push = $state(false);

  // A new selection resets the editing state along with the message.
  $effect(() => {
    store.selectedAgentId;
    messageEdited = false;
    push = false;
  });

  const running = $derived(store.selectedAgent?.state === "running");

  /**
   * Whether the agent's last turn ran in plan mode. An empty diff then isn't a
   * turn that achieved nothing — it's a turn that was asked not to write.
   */
  const planning = $derived(store.selectedAgent?.permission_mode === "plan");

  const totals = $derived.by(() => {
    const files = review.diff?.files ?? [];
    return files.reduce(
      (acc, f) => ({
        insertions: acc.insertions + (f.insertions ?? 0),
        deletions: acc.deletions + (f.deletions ?? 0),
      }),
      { insertions: 0, deletions: 0 },
    );
  });

  const patchFiles = $derived(parsePatch(review.diff?.patch ?? ""));

  const highlighted = $derived(
    patchFiles.files.map((pf) => highlightPatch(pf.lines, languageOf(pf.path))),
  );

  async function doCommit() {
    if (await review.commit(message)) messageEdited = false;
  }

  /** Which branch the merge picker is pointing at. */
  let target = $state("");

  /**
   * Which project we last asked for branches. A plain `let`, not `$state`, so
   * reading it here doesn't make this effect depend on itself — and so a fresh
   * `agents` array (they arrive constantly) doesn't re-run git on every event.
   */
  let branchesFor: string | null = null;

  $effect(() => {
    const project = store.selectedAgent?.project_id ?? null;
    if (project === branchesFor) return;
    branchesFor = project;
    review.loadBranches();
  });

  const mergedInto = $derived(review.mergedInto);

  /** What is left to merge into, which is what the picker offers. */
  const targets = $derived(review.targets);

  // Point at the default target, and follow the list rather than leaving a name
  // in the picker that no longer exists.
  $effect(() => {
    if (!targets.includes(target)) target = review.defaultTarget ?? "";
  });

  /** Only a target that tracks a remote has anywhere to push. */
  const pushable = $derived(review.branches?.pushable.includes(target) ?? false);

  /**
   * The last merge isn't on the remote yet. A record from before `unpushed`
   * existed says so only by its push error.
   */
  const unpushed = $derived(
    !!(store.selectedAgent?.unpushed || store.selectedAgent?.push_error),
  );

  // Only committed work merges, and not out from under a working agent.
  const canMerge = $derived(
    !running && !review.merging && !(review.diff?.uncommitted ?? false) && targets.length > 0,
  );

  /**
   * Whether this work has landed and nothing has happened since: some branch
   * has the agent's tip, and the worktree is clean. `merged_into` empties the
   * moment the agent commits again, so this reads as "merged, no new changes"
   * — and a merge into whatever other branches remain is not what the user
   * came here for. See `merged_into` in api.ts.
   */
  const settled = $derived(
    mergedInto.length > 0 && !(review.diff?.uncommitted ?? false),
  );

  /** The last merge of this agent hit conflicts, and nobody has taken them on yet. */
  const conflict = $derived(
    store.selectedAgentId ? review.conflicts[store.selectedAgentId] : undefined,
  );

  /** A resolver already working on this agent's conflicts. */
  const resolver = $derived(
    store.selectedAgentId ? review.resolverFor(store.selectedAgentId) : null,
  );

  /**
   * The agent this one is resolving a merge for, while that merge is still
   * pending — it finishes on its own when this agent's turn completes.
   */
  const resolvingFor = $derived.by(() => {
    const r = store.selectedAgent?.resolves;
    if (!r || store.selectedAgent?.merged_at) return null;
    const agent = store.agents.find((a) => a.id === r.agent_id);
    return agent ? { agent, target: r.target } : null;
  });
</script>

<div class="diff-pane">
  {#if review.loading && !review.diff}
    <div class="hint">Reading the worktree…</div>
  {:else if review.error && !review.diff}
    <div class="diff-error">{review.error}</div>
  {:else if review.diff}
    {@const diff = review.diff}

    <div class="summary">
      <div class="counts">
        <span class="n">{diff.files.length}</span>
        file{diff.files.length === 1 ? "" : "s"}
        <span class="ins">+{totals.insertions}</span>
        <span class="del">−{totals.deletions}</span>
      </div>
      <div class="summary-right">
        <span class="mono base">base {diff.base.slice(0, 7)}</span>
        <button onclick={() => review.load()} disabled={review.loading}>
          {review.loading ? "Refreshing…" : "Refresh"}
        </button>
      </div>
    </div>

    <div class="diff-body">
      {#if review.error}
        <div class="diff-error">{review.error}</div>
      {/if}

      {#if diff.files.length === 0 && diff.commits.length === 0}
        <div class="hint">
          {#if planning}
            This agent is in plan mode — it reads and proposes, so there is
            nothing here to commit. Switch it to YOLO and reply to set it
            working.
          {:else}
            This agent changed nothing in its worktree.
          {/if}
        </div>
      {/if}

      {#if diff.commits.length > 0}
        <div class="commits">
          <div class="section-label">
            {diff.commits.length} commit{diff.commits.length === 1 ? "" : "s"} on
            <code>{store.selectedAgent?.branch}</code>
          </div>
          <div class="scroll-list">
            {#each diff.commits as c (c.sha)}
              <div class="commit">
                <code class="sha">{c.sha}</code>
                <span class="subject">{c.subject}</span>
              </div>
            {/each}
          </div>
        </div>
      {/if}

      {#if diff.uncommitted}
        <div class="commit-box">
          <div class="commit-box-head">
            <span class="warn-dot"></span>
            <strong>Uncommitted work</strong>
            <span class="sub">Discarding now would lose it.</span>
          </div>
          <textarea
            class="textarea"
            bind:value={message}
            oninput={() => (messageEdited = true)}
            rows="2"
            placeholder="Commit message"
            disabled={running || review.committing}
          ></textarea>
          <div class="commit-box-foot">
            {#if running}
              <span class="sub">Stop the agent before committing — it is still writing.</span>
            {:else}
              <span class="sub">Stages everything in the worktree onto the agent's branch.</span>
            {/if}
            <button
              class="btn btn-warning btn-lg"
              disabled={running || review.committing || message.trim().length === 0}
              onclick={doCommit}
            >
              {review.committing ? "Committing…" : "Commit all changes"}
            </button>
          </div>
        </div>
      {/if}

      {#if unpushed && store.selectedAgent}
        {@const failed = store.selectedAgent.push_error}
        <!-- The merge landed here but not on the remote, which is where other
             machines look for it. Only a failed push is a warning: not
             pushing is the default. -->
        <div class="merge-box" class:push-failed={!!failed}>
          <div class="merge-box-head">
            <strong>{failed ? "Push failed" : "Not pushed"}</strong>
            <span class="sub">
              merged into <code>{store.selectedAgent.merged_branch}</code> on this machine only
            </span>
          </div>
          <div class="merge-box-foot">
            {#if failed}
              <span class="sub why">{failed}</span>
            {:else}
              <span class="sub">Push it to the remote when your other machines should have it.</span>
            {/if}
            <div class="merge-controls">
              <button class="btn btn-primary" disabled={review.pushing} onclick={() => review.push()}>
                {review.pushing ? "Pushing…" : "Push"}
              </button>
            </div>
          </div>
        </div>
      {/if}

      {#if diff.commits.length > 0}
        {#if conflict}
          <!-- The merge aborted, so the project is as it was. Offer the one
               way forward the app has: hand the collision to an agent. -->
          <div class="merge-box conflict">
            <div class="merge-box-head">
              <span class="danger-dot"></span>
              <strong>Conflicts merging into <code>{conflict.target}</code></strong>
              <span class="sub">the project is unchanged</span>
            </div>
            <ul class="conflict-files">
              {#each conflict.files as f (f)}
                <li class="mono">{f}</li>
              {/each}
            </ul>
            <div class="merge-box-foot">
              <span class="sub">
                A new agent merges <code>{conflict.target}</code> into a copy of this
                branch and settles the conflicts. When it finishes, the merge
                completes and both agents move to Delivered.
              </span>
              <div class="merge-controls">
                <button
                  class="btn"
                  disabled={review.resolving}
                  onclick={() => review.dismissConflict(store.selectedAgentId!)}
                >
                  Dismiss
                </button>
                <button
                  class="btn btn-primary"
                  disabled={running || review.resolving}
                  onclick={() => review.resolveConflict()}
                >
                  {review.resolving ? "Starting…" : "Resolve with an agent"}
                </button>
              </div>
            </div>
          </div>
        {:else if resolver}
          <div class="merge-box">
            <div class="merge-box-head">
              <span class={`status-dot status-${resolver.state}`}></span>
              <strong>Resolving conflicts</strong>
              <span class="sub">with <code>{resolver.resolves?.target}</code></span>
            </div>
            <div class="merge-box-foot">
              <span class="sub">
                {#if resolver.state === "running"}
                  {store.agentName(resolver)} is on it. This work merges with its
                  resolution when it finishes.
                {:else}
                  {store.agentName(resolver)} stopped before the merge went through.
                  Open it to see why, and reply to finish the job.
                {/if}
              </span>
              <div class="merge-controls">
                <button class="btn" onclick={() => store.showAgent(resolver.id)}>Open resolver</button>
              </div>
            </div>
          </div>
        {:else if settled}
          <!-- Already merged with nothing new since: say where the work went
               instead of offering a control there is no reason to use. -->
          <div class="merge-box merged">
            <div class="merge-box-head">
              <strong>Merged</strong>
              <span class="sub">this work is in <code>{mergedInto.join(", ")}</code></span>
            </div>
          </div>
        {:else}
          <div class="merge-box">
            <div class="merge-box-head">
              <strong>Merge this work</strong>
              {#if resolvingFor}
                <span class="sub">
                  resolving
                  <button class="link" onclick={() => store.showAgent(resolvingFor.agent.id)}
                    >{store.agentName(resolvingFor.agent)}</button
                  >
                  — merges into <code>{resolvingFor.target}</code> by itself when it finishes
                </span>
              {:else if mergedInto.length > 0}
                <span class="sub">already in <code>{mergedInto.join(", ")}</code></span>
              {/if}
            </div>
            <div class="merge-box-foot">
              {#if running}
                <span class="sub">Stop the agent before merging — it is still writing.</span>
              {:else if diff.uncommitted}
                <span class="sub">Commit the loose work first — only commits merge.</span>
              {:else if targets.length === 0}
                <span class="sub">No branches to merge into.</span>
              {:else}
                <span class="sub">
                  Merges <code>{store.selectedAgent?.branch}</code> in, keeping it one commit.
                </span>
              {/if}
              <div class="merge-controls">
                <BranchPicker
                  bind:value={target}
                  options={targets}
                  current={review.branches?.current ?? null}
                  disabled={!canMerge}
                  label="Branch to merge into"
                />
                {#if pushable}
                  <label class="switch" title="Push {target} to the remote once merged">
                    <input type="checkbox" role="switch" bind:checked={push} disabled={!canMerge} />
                    <span class="track" aria-hidden="true"><span class="thumb"></span></span>
                    Push
                  </label>
                {/if}
                <button
                  class="btn btn-primary"
                  disabled={!canMerge || target.length === 0}
                  onclick={() => review.merge(target, push && pushable)}
                >
                  {#if review.merging}
                    {push && pushable ? "Merging and pushing…" : "Merging…"}
                  {:else}
                    {push && pushable ? "Merge and push" : "Merge"}
                  {/if}
                </button>
              </div>
            </div>
          </div>
        {/if}
      {/if}

      {#if diff.files.length > 0}
        <div class="files scroll-list">
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

      {#each patchFiles.files as pf, fi (pf.path)}
        <details class="patch-file" open>
          <summary class="mono">{pf.path}</summary>
          <div class="patch">
            {#each pf.lines as line, i (i)}
              {@const spans = highlighted[fi]?.[i]}
              <!-- Coloured, a line keeps its +/- and its tint to say what
                   changed, and the code takes its syntax colours. -->
              <div class={`line ${line.kind}`} class:coloured={!!spans}
                >{#if spans}<span class="sign">{line.text[0]}</span><CodeSpans
                    {spans}
                  />{:else}{line.text || " "}{/if}</div
              >
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
    </div>
  {/if}
</div>

<style>
  .diff-pane {
    flex: 1;
    min-height: 0;
    min-width: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  /* Everything below the summary bar scrolls, so the counts and Refresh
     stay reachable however long the patch runs. */
  .diff-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
    /* The last lines scroll clear of a phone's home indicator. */
    padding: 0.85rem var(--pad-x) calc(2rem + var(--safe-bottom));
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
  }

  /* A long list gets its own scrollbar rather than pushing the rest away. */
  .scroll-list {
    max-height: 13rem;
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  .diff-pane > .diff-error {
    margin: 1rem var(--pad-x);
  }

  .hint {
    color: var(--fg-muted);
    font-style: italic;
    text-align: center;
    padding: 3rem 1rem;
    font-size: var(--text-lg);
  }

  .mono {
    font-family: var(--font-mono);
  }

  code {
    font-family: var(--font-mono);
    background: var(--code-bg);
    padding: 0.05em 0.35em;
    border-radius: var(--radius-xs);
  }

  .summary {
    flex: none;
    padding: 0.7rem var(--pad-x);
    background: var(--surface);
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.4rem 1rem;
    font-size: var(--text-md);
    color: var(--fg-muted);
  }

  .summary .n {
    color: var(--fg);
    font-weight: var(--weight-semibold);
  }

  .summary-right {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    margin-left: auto;
  }

  .base {
    font-size: var(--text-sm);
  }

  .summary button {
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    background: var(--surface);
    color: var(--fg);
    padding: 0.28rem 0.7rem;
    font-size: var(--text-sm);
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
    color: var(--success);
    font-family: var(--font-mono);
  }

  .del {
    color: var(--danger-text);
    font-family: var(--font-mono);
  }

  .section-label {
    font-size: var(--text-sm);
    color: var(--fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin-bottom: var(--space-3);
  }

  .section-label code {
    text-transform: none;
    letter-spacing: 0;
  }

  .commits {
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--panel-bg);
    padding: 0.6rem 0.8rem;
    min-width: 0;
  }

  .commit {
    display: flex;
    gap: var(--space-4);
    align-items: baseline;
    font-size: var(--text-md);
    padding: 0.12rem 0;
    min-width: 0;
  }

  .commit .sha {
    color: var(--accent);
    background: transparent;
    padding: 0;
    flex: none;
  }

  .commit .subject {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .commit-box {
    border: 1px solid var(--warning-soft-border);
    background: var(--warning-soft-bg);
    border-radius: var(--radius-md);
    padding: 0.7rem 0.85rem;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .commit-box-head {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    font-size: var(--text-lg);
  }

  .warn-dot {
    width: 7px;
    height: 7px;
    border-radius: var(--radius-circle);
    background: var(--warning);
    flex: none;
  }

  .danger-dot {
    width: 7px;
    height: 7px;
    border-radius: var(--radius-circle);
    background: var(--danger);
    flex: none;
    align-self: center;
  }

  .sub {
    color: var(--fg-muted);
    font-size: var(--text-sm);
  }

  /* An agent's name inside a line of text, that opens the agent. */
  .sub button.link {
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    color: var(--accent);
    cursor: pointer;
  }

  .sub button.link:hover {
    text-decoration: underline;
  }

  /* Only what makes this field different from the app's `.textarea`: it is
     a commit message, so it grows by the handle rather than by the line. */
  .commit-box :global(textarea) {
    resize: vertical;
    font-size: var(--text-lg);
  }

  .commit-box-foot {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem 1rem;
  }

  .commit-box-foot .sub {
    flex: 1 1 12rem;
  }

  .commit-box-foot :global(button) {
    flex: none;
  }

  /* Deliberately quieter than the commit box: merging is the ordinary next
     step, not a warning that work is at risk. */
  .merge-box {
    border: 1px solid var(--border);
    background: var(--surface);
    border-radius: var(--radius-md);
    padding: 0.7rem 0.85rem;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  /* A merge that didn't go through is the one thing here asking for a
     decision, so it takes the danger tint rather than the neutral one. */
  .merge-box.conflict {
    border-color: var(--danger-soft-border);
    background: var(--danger-soft-bg);
  }

  .conflict-files {
    margin: 0;
    padding: 0 0 0 1.1rem;
    font-size: var(--text-md);
    max-height: 8rem;
    overflow: auto;
  }

  .merge-box .status-dot {
    align-self: center;
  }

  /* Quieter still once there is nothing to do but read where it went. */
  .merge-box.merged {
    padding: 0.55rem 0.85rem;
  }

  .merge-box.push-failed {
    border-color: var(--warning-soft-border);
    background: var(--warning-soft-bg);
  }

  .merge-box.push-failed .why {
    overflow-wrap: anywhere;
  }

  .merge-box-head {
    display: flex;
    align-items: baseline;
    gap: var(--space-4);
    font-size: var(--text-lg);
  }

  .merge-box-foot {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem 1rem;
  }

  .merge-box-foot .sub {
    flex: 1 1 12rem;
  }

  .merge-controls {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex: none;
  }

  /* Its label reads as part of the controls, at their size. */
  .merge-controls .switch {
    font-size: var(--control-font-size);
    margin: 0 var(--space-2);
  }

  .files {
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
  }

  .file-row {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding: 0.35rem 0.7rem;
    font-size: var(--text-md);
    border-bottom: 1px solid var(--border);
    min-width: 0;
  }

  .file-row .ins,
  .file-row .del {
    flex: none;
    font-variant-numeric: tabular-nums;
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
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    font-weight: var(--weight-bold);
    border-radius: var(--radius-xs);
    background: var(--code-bg);
    color: var(--fg-muted);
    flex: none;
  }

  .badge-a { color: var(--success); }
  .badge-m { color: var(--warning-text); }
  .badge-d { color: var(--danger-text); }

  .binary {
    color: var(--fg-muted);
    font-size: var(--text-sm);
    font-style: italic;
  }

  .patch-file {
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    min-width: 0;
  }

  .patch-file summary {
    cursor: pointer;
    padding: 0.4rem 0.7rem;
    background: var(--float-bg, var(--panel-bg));
    -webkit-backdrop-filter: var(--float-filter, none);
    backdrop-filter: var(--float-filter, none);
    font-size: var(--text-md);
    color: var(--fg-muted);
    overflow-wrap: anywhere;
    border-radius: var(--radius-sm) var(--radius-sm) 0 0;
    /* Stays put while its own patch scrolls past underneath. */
    position: sticky;
    top: 0;
    z-index: var(--z-base);
  }

  .patch-file[open] summary {
    border-bottom: 1px solid var(--border);
  }

  .patch {
    /* Code lines keep their shape: this scrolls both ways on its own, so one
       big file can't swallow the pane. */
    overflow: auto;
    overscroll-behavior: contain;
    max-height: clamp(10rem, 50vh, 32rem);
    border-radius: 0 0 var(--radius-sm) var(--radius-sm);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    line-height: var(--leading-relaxed);
  }

  .line {
    padding: 0 0.7rem;
    white-space: pre;
    min-width: max-content;
  }

  .line.add {
    background: var(--diff-add-bg);
    color: var(--diff-add-fg);
  }

  .line.del {
    background: var(--diff-del-bg);
    color: var(--diff-del-fg);
  }

  .line.hunk {
    background: var(--code-bg);
    color: var(--fg-muted);
  }

  .line.coloured {
    color: var(--fg);
  }

  .line.add .sign { color: var(--diff-add-fg); }
  .line.del .sign { color: var(--diff-del-fg); }

  .diff-error,
  .clipped {
    border-radius: var(--radius-md);
    padding: 0.55rem 0.85rem;
    font-size: var(--text-md);
  }

  .diff-error {
    background: var(--danger-soft-bg);
    border: 1px solid var(--danger-soft-border);
    color: var(--fg);
    font-family: var(--font-mono);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .clipped {
    background: var(--panel-bg);
    border: 1px solid var(--border);
    color: var(--fg-muted);
    overflow-wrap: anywhere;
  }
</style>
