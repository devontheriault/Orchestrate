<script module lang="ts">
  import { SvelteMap } from "svelte/reactivity";

  // Module-level, so they outlive the transcript: it unmounts whenever the
  // Diff tab is open, and coming back to it shouldn't start from scratch.

  /**
   * How far from the bottom still counts as being at the bottom: scrolling
   * back down to within this slack picks the stream up again.
   */
  const FOLLOW_SLACK = 120;

  /**
   * The page holds a window of the Rows rather than all of them: the newest
   * TAIL while following, and never more than MOST however far back the user
   * reads. A long transcript is thousands of Rows, and every one on the page
   * made opening an agent, each event and each layout slower — whether it
   * was in view or not.
   */
  const TAIL = 100;
  const MOST = 200;

  /**
   * How many Rows come onto the page, and as many leave it at the other end,
   * each time the user scrolls near an edge of the window. Small enough that
   * building them doesn't stall the scroll.
   */
  const PAGE = 25;

  /** How near an edge of the page, in pixels, slides the window that way. */
  const EDGE_SLACK = 600;

  /**
   * Whether the transcript is following the newest output. Only the user
   * scrolling up lets go of it — new output never does. Measuring the distance
   * from the bottom once output arrives can't tell the two apart: by then the
   * DOM already holds it, so one large chunk reads as the user having scrolled
   * away. The jump-to-bottom button shows exactly while this is off.
   */
  let following = $state(true);
  let lastScrollTop = 0;

  /**
   * Cards the user has opened or folded by hand, by agent and key. Kept here
   * rather than in the DOM because a card leaves the page when the window
   * slides past it, and should come back the way the user left it.
   */
  const toggled = new SvelteMap<string, boolean>();
</script>

<script lang="ts">
  import { tick } from "svelte";
  import { store } from "$lib/state/store.svelte";
  import { formatDuration, formatTokens } from "$lib/format";
  import Markdown from "$lib/markdown/Markdown.svelte";
  import LinkedText from "$lib/markdown/LinkedText.svelte";
  import Attachments from "$lib/agent/Attachments.svelte";
  import HighlightedCode from "$lib/code/HighlightedCode.svelte";
  import NumberedCode from "$lib/code/NumberedCode.svelte";
  import Copyable from "$lib/code/Copyable.svelte";
  import ShellCommand from "./ShellCommand.svelte";
  import { buildRows, humanize, keepUnchanged, systemLabel, type Row, type ToolCall } from "./rows";
  import {
    bashCommand,
    bashOutputLanguage,
    callTarget,
    elapsedLabel,
    fileChange,
    groupElapsed,
    groupTargets,
    readLines,
    resultText,
    truncate,
  } from "./toolCalls";

  /** The blank page for an agent that hasn't been spawned yet. */
  const drafting = $derived(store.drafting && !store.selectedAgent);

  let streamEl: HTMLDivElement | undefined = $state();

  /**
   * Rebuilt on every event, but keeping last time's Row wherever it came out
   * the same, so only what the event changed re-renders.
   */
  let lastRows: Row[] = [];
  const rows: Row[] = $derived.by(
    () => (lastRows = keepUnchanged(lastRows, buildRows(store.eventsForSelected))),
  );

  /**
   * The first Row on the page. Infinity while following, so the page slides
   * along with the newest TAIL Rows. Letting go of the stream pins it where
   * it is, so nothing leaves the top of the page under the user's eyes; the
   * page then ends MOST Rows on, and output past that waits for the user to
   * scroll down to it.
   */
  let start = $state(Infinity);
  const shownFrom = $derived(Math.max(0, Math.min(start, rows.length - TAIL)));
  const shownTo = $derived(Math.min(rows.length, shownFrom + MOST));
  const shown = $derived(
    shownFrom > 0 || shownTo < rows.length ? rows.slice(shownFrom, shownTo) : rows,
  );

  function follow(on: boolean) {
    if (on === following) return;
    following = on;
    start = on ? Infinity : shownFrom;
  }

  function onScroll() {
    const el = streamEl;
    if (!el) return;
    const fromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
    // The bottom of the page is only the newest output once the window
    // reaches it.
    if (fromBottom < FOLLOW_SLACK && shownTo === rows.length) follow(true);
    // Our own scrolls only ever go down, and content growing below doesn't
    // move scrollTop at all, so a scroll upward is the user's doing.
    else if (el.scrollTop < lastScrollTop) follow(false);
    lastScrollTop = el.scrollTop;
    if (following) return;
    if (el.scrollTop < EDGE_SLACK && shownFrom > 0) void slide(Math.max(0, shownFrom - PAGE));
    else if (fromBottom < EDGE_SLACK && shownTo < rows.length)
      void slide(Math.min(shownFrom + PAGE, rows.length - MOST));
  }

  /**
   * Moves the window to start at `to`, keeping what's on screen where it is:
   * Rows coming or going above it would otherwise move it by their height.
   * Done by hand because WebKit doesn't anchor scrolling itself.
   */
  let sliding = false;
  async function slide(to: number) {
    const el = streamEl;
    if (!el || sliding) return;
    sliding = true;
    // The first Row on both the page now and the page to come.
    const anchor = el.querySelectorAll(":scope > .block")[Math.max(0, to - shownFrom)];
    const top = anchor?.getBoundingClientRect().top ?? 0;
    start = to;
    await tick();
    if (anchor?.isConnected) el.scrollTop += anchor.getBoundingClientRect().top - top;
    lastScrollTop = el.scrollTop;
    sliding = false;
  }

  function stickToBottom() {
    requestAnimationFrame(() => {
      if (following && streamEl) streamEl.scrollTop = streamEl.scrollHeight;
    });
  }

  async function jumpToBottom() {
    const el = streamEl;
    if (!el) return;
    follow(true);
    // Scroll to where the bottom is once the page has dropped its older Rows.
    await tick();
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    el.scrollTo({ top: el.scrollHeight, behavior: reduced ? "auto" : "smooth" });
  }

  // New output scrolls itself into view unless the user has scrolled up.
  // Keyed on the Rows rather than the events: most events change nothing on
  // screen, and those shouldn't cost a scroll.
  $effect(() => {
    if (!streamEl || rows.length === 0) return;
    if (following) stickToBottom();
  });

  // The pane shrinking (a taller composer, a resized window) would otherwise
  // push the newest output out of sight without anything being scrolled.
  function followResizes(el: HTMLDivElement) {
    const ro = new ResizeObserver(() => {
      if (following) el.scrollTop = el.scrollHeight;
    });
    ro.observe(el);
    return () => ro.disconnect();
  }

  // On selection change, always jump to the bottom of the new stream.
  $effect(() => {
    const id = store.selectedAgentId;
    if (!streamEl || !id) return;
    following = true;
    start = Infinity;
    stickToBottom();
  });

  /**
   * The newest Bash call in the transcript. It opens itself, so the command
   * being run — and then what it printed — is in view without a click, and
   * stays open until the next Bash call takes its place.
   */
  const lastBashKey = $derived.by(() => {
    const row = rows.findLast((r) => r.kind === "tools" && r.name === "Bash");
    return row?.kind === "tools" ? row.calls.at(-1)?.key : undefined;
  });

  /**
   * The Write calls since the user's last prompt. They open themselves, so a
   * file's new contents are in view as soon as it's written. An earlier
   * Turn's stay folded: each is a whole file, highlighted, and a long
   * transcript holds dozens.
   */
  const turnWrites = $derived.by(() => {
    const keys = new Set<string>();
    for (let i = rows.length - 1; i >= 0 && rows[i].kind !== "prompt"; i--) {
      const row = rows[i];
      if (row.kind === "tools" && row.name === "Write") for (const c of row.calls) keys.add(c.key);
    }
    return keys;
  });

  /** Calls that open without a click: the newest Bash call, and this Turn's Writes. */
  function opensItself(c: ToolCall): boolean {
    return c.key === lastBashKey || turnWrites.has(c.key);
  }

  /**
   * Whether the card under `key` is open: as the user last left it, or else
   * `auto`. What's inside a card is only built while it is open — a
   * transcript holds hundreds of them, and one Read alone can be a thousand
   * highlighted lines nobody asked to see.
   */
  function isOpen(key: string, auto: boolean): boolean {
    return toggled.get(`${store.selectedAgentId} ${key}`) ?? auto;
  }

  /**
   * Keeps a `<details>` following `want`, but only touches it when `want`
   * flips. Rows are rebuilt on every event, and a plain `open={…}` would be
   * re-applied each time — folding up whatever the user had opened by hand.
   */
  const applied = new WeakMap<HTMLDetailsElement, boolean>();
  function autoOpen(want: boolean) {
    return (el: HTMLDetailsElement) => {
      if (applied.get(el) === want) return;
      applied.set(el, want);
      el.open = want;
    };
  }

  /** Records the user opening or folding the card under `key`. */
  function remember(key: string) {
    return (e: Event) => {
      const el = e.currentTarget as HTMLDetailsElement;
      // `autoOpen` toggles it too, and that's no choice of the user's.
      if (el.open === applied.get(el)) return;
      applied.set(el, el.open);
      toggled.set(`${store.selectedAgentId} ${key}`, el.open);
    };
  }
</script>

<!-- A call's input as shown when expanded. A command, or code going into a
     file, reads better laid out and coloured than JSON-escaped. -->
<!-- What came back from a call. A file that was read shows as code, beside
     its line numbers, and so does a file or diff a command printed. -->
{#snippet callResult(c: ToolCall)}
  {@const read = readLines(c)}
  {@const printed = bashOutputLanguage(c)}
  <div class="call-result">
    {#if read}
      <Copyable flush text={read.lines.map((l) => l.text).join("\n")}>
        <pre class="read"><NumberedCode lines={read.lines} lang={read.lang} /></pre>
      </Copyable>
      {#if read.rest}<pre><LinkedText text={read.rest} /></pre>{/if}
    {:else}
      {@const text = resultText(c)}
      <Copyable flush {text}>
        {#if printed}
          <pre class="read"><HighlightedCode code={text} lang={printed} /></pre>
        {:else}
          <pre><LinkedText {text} /></pre>
        {/if}
      </Copyable>
    {/if}
  </div>
{/snippet}

<!-- A call's input and result, for a call whose card is open. -->
{#snippet callBody(c: ToolCall)}
  {@render callInput(c)}
  {#if c.hasResult}
    {@render callResult(c)}
  {/if}
{/snippet}

{#snippet callInput(c: ToolCall)}
  {@const command = bashCommand(c)}
  {@const file = fileChange(c)}
  {#if command !== undefined}
    <Copyable flush text={command}><ShellCommand {command} /></Copyable>
  {:else if file}
    {#if file.before !== undefined}
      {#if file.everywhere}<div class="edit-note">every occurrence</div>{/if}
      <Copyable flush text={file.before}>
        <pre class="edit-before"><HighlightedCode code={file.before} lang={file.lang} /></pre>
      </Copyable>
      <Copyable flush text={file.after}>
        <pre class="edit-after"><HighlightedCode code={file.after} lang={file.lang} /></pre>
      </Copyable>
    {:else}
      <Copyable flush text={file.after}>
        <pre><HighlightedCode code={file.after} lang={file.lang} /></pre>
      </Copyable>
    {/if}
  {:else}
    {@const json = JSON.stringify(c.input, null, 2)}
    <Copyable flush text={json}><pre>{json}</pre></Copyable>
  {/if}
{/snippet}

<div class="stream-wrap">
  <div class="stream" bind:this={streamEl} onscroll={onScroll} {@attach followResizes}>
    {#if store.selectedAgent?.state === "failed" && store.selectedAgent.fail_reason}
      <div class="fail-banner">
        <span class="label">Failed</span>
        <span class="reason"><LinkedText text={store.selectedAgent.fail_reason} /></span>
      </div>
    {/if}

    {#if drafting}
      <div class="hint draft-hint">
        <p>This agent's output will appear here.</p>
        <p class="sub">
          Describe the task below and spawn it — it gets a fresh worktree and
          branch of its own.
        </p>
      </div>
    {:else if !store.selectedAgent}
      <div class="hint">Open a project and pick one of its agents to see its output.</div>
    {:else if store.eventsForSelected.length === 0}
      <div class="hint">
        {store.selectedAgent.state === "running"
          ? "Waiting for output…"
          : "No events on record."}
      </div>
    {:else}
      {#each shown as row (row.key)}
        {#if row.kind === "prompt"}
          <!-- One line on purpose: the block is pre-wrap, so any whitespace
               between these tags would show up as blank space. -->
          <div class="block prompt-block">{#if row.attachments.length}<div class="prompt-files"><Attachments paths={row.attachments} /></div>{/if}<LinkedText text={row.text} /></div>
        {:else if row.kind === "text"}
          <div class="block text"><Markdown text={row.text} /></div>
        {:else if row.kind === "tools"}
          {@const open = isOpen(row.key, row.calls.some(opensItself))}
          <details class="block tool" {@attach autoOpen(open)} ontoggle={remember(row.key)}>
            <summary>
              <span class="tool-name">→ {row.name}</span>
              {#if row.calls.length > 1}<span class="count">×{row.calls.length}</span>{/if}
              {#if groupTargets(row.calls)}
                <span class="targets">{groupTargets(row.calls)}</span>
              {/if}
              {#if row.calls.some((c) => !c.hasResult)}
                <span class="status-dot status-running running-dot" title="still running"></span>
              {/if}
              {#if groupElapsed(row.calls)}
                <span class="elapsed">{groupElapsed(row.calls)}</span>
              {/if}
            </summary>
            {#if open && row.calls.length === 1}
              {@render callBody(row.calls[0])}
            {:else if open}
              <!-- A run of same-tool calls: the group is one row, each
                   call inside it still opens on its own. The row's own key
                   is its first call's, so the calls take keys of their own. -->
              {#each row.calls as c (c.key)}
                {@const callOpen = isOpen(`${c.key} call`, opensItself(c))}
                <details
                  class="call"
                  {@attach autoOpen(callOpen)}
                  ontoggle={remember(`${c.key} call`)}
                >
                  <summary>
                    {truncate(callTarget(c.name, c.input), 80) || c.name}
                    {#if !c.hasResult}<span class="status-dot status-running running-dot"></span>{/if}
                    {#if elapsedLabel(c)}<span class="elapsed">{elapsedLabel(c)}</span>{/if}
                  </summary>
                  {#if callOpen}{@render callBody(c)}{/if}
                </details>
              {/each}
            {/if}
          </details>
        {:else if row.kind === "thinking"}
          {@const open = isOpen(row.key, false)}
          <details class="block thinking" {@attach autoOpen(open)} ontoggle={remember(row.key)}>
            <summary
              >thinking{#if row.parts.length > 1}<span class="count"
                  >×{row.parts.length}</span
                >{/if}</summary
            >
            {#if open}
              <div class="thinking-body"><Markdown text={row.parts.join("\n\n")} /></div>
            {/if}
          </details>
        {:else if row.kind === "system"}
          <div class="block system">{systemLabel(row.subtype)}</div>
        {:else if row.kind === "notice"}
          <div class="block system notice"><Markdown text={row.text} /></div>
        {:else if row.kind === "result"}
          <div class="block done" class:err={row.is_error}>
            {row.is_error ? "✗ error" : "✓ done"}
            {#if row.duration_ms != null}
              — {formatDuration(row.duration_ms)}
            {/if}
            {#if row.output_tokens != null}
              · {formatTokens(row.output_tokens)} tok
            {/if}
            {#if row.is_error && row.result}
              — <LinkedText text={row.result} />
            {/if}
          </div>
        {:else}
          {@const open = isOpen(row.key, false)}
          <details class="block raw-detail" {@attach autoOpen(open)} ontoggle={remember(row.key)}>
            <summary>{humanize(row.type ?? "") || "Unrecognized event"}</summary>
            {#if open}
              {@const json = JSON.stringify(row.event, null, 2)}
              <Copyable flush text={json}><pre>{json}</pre></Copyable>
            {/if}
          </details>
        {/if}
      {/each}
    {/if}
  </div>
  {#if !following}
    <!-- Only while the transcript has been scrolled away from: the way
         back down is one click, in the gap above the composer. -->
    <button
      class="to-bottom"
      onclick={jumpToBottom}
      aria-label="Jump to the latest output"
      title="Jump to the latest output"
    >
      <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
        <path
          d="M8 3v9M8 12l-4-4M8 12l4-4"
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

<style>
  /* Holds the transcript and the jump-to-bottom button that floats over it. */
  .stream-wrap {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .stream {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
    /* Adding earlier Rows keeps the view still by hand (`showEarlier`); the
       browser's own anchoring, where there is any, would move it twice. */
    overflow-anchor: none;
    padding: 1rem var(--pad-x) 2rem;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  /* Centred on the transcript's own edge, just above the composer: the button
     sits where the newest output is, which is where the eye already is. */
  .to-bottom {
    position: absolute;
    bottom: 0.6rem;
    left: 50%;
    transform: translateX(-50%);
    width: 2rem;
    height: 2rem;
    display: grid;
    place-items: center;
    border: 1px solid var(--border);
    border-radius: var(--radius-circle);
    background: var(--float-bg, var(--panel-bg));
    -webkit-backdrop-filter: var(--float-filter, none);
    backdrop-filter: var(--float-filter, none);
    color: var(--fg-muted);
    box-shadow: var(--shadow-sm);
    cursor: pointer;
    padding: 0;
    animation: to-bottom-in 0.12s ease;
  }

  .to-bottom:hover {
    border-color: var(--accent);
    color: var(--accent);
  }

  @keyframes to-bottom-in {
    from { opacity: 0; transform: translate(-50%, 0.3rem); }
    to { opacity: 1; transform: translate(-50%, 0); }
  }

  .hint {
    color: var(--fg-muted);
    font-style: italic;
    text-align: center;
    padding: 3rem 1rem;
    font-size: var(--text-lg);
  }

  /* The blank page is mostly empty on purpose: it's the transcript, waiting.
     Centre the little it does say in the space the output will fill. */
  .draft-hint {
    margin: auto 0;
    font-style: normal;
  }

  .draft-hint p {
    margin: 0;
  }

  .draft-hint .sub {
    margin-top: var(--space-3);
    font-size: var(--text-md);
    max-width: var(--measure);
    margin-left: auto;
    margin-right: auto;
  }

  .block {
    font-size: var(--text-lg);
    line-height: var(--leading-relaxed);
    min-width: 0;
  }

  /* Claude answers in markdown, so the transcript renders it as a document
     rather than as the source. `Markdown` brings its own spacing. */
  .text {
    padding: 0.15rem 0;
    /* Prose stops at a readable measure however wide the window gets. */
    max-width: min(100%, var(--measure));
  }

  /* The user's side of the conversation: indented, so turns are easy to find
     when scanning a long transcript. */
  .prompt-block {
    align-self: flex-start;
    max-width: min(100%, var(--measure));
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    background: var(--selected);
    border-left: 2px solid var(--accent);
    border-radius: 0 var(--radius-md) var(--radius-md) 0;
    padding: 0.45rem 0.7rem;
    margin: 0.45rem 0 0.2rem;
    font-size: var(--text-lg);
  }

  /* Above the text, as they sat above it in the box it was typed into. */
  .prompt-files {
    margin-bottom: 0.4rem;
    white-space: normal;
  }

  details.block {
    background: var(--panel-bg);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    padding: 0.4rem 0.75rem;
  }

  details.block summary {
    cursor: pointer;
    font-family: var(--font-mono);
    font-size: var(--text-md);
    color: var(--fg-muted);
    overflow-wrap: anywhere;
  }

  details.block[open] summary { margin-bottom: var(--space-3); }

  details.block pre {
    background: transparent;
    margin: 0;
    padding: 0;
    font-size: var(--text-sm);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    color: var(--fg);
    /* A very long line scrolls inside the card instead of stretching it. */
    max-height: min(60vh, 34rem);
    overflow: auto;
  }

  /* Thinking is prose too, but a long stretch of it scrolls inside its card
     instead of burying the answer that follows. */
  .thinking-body {
    font-size: var(--text-md);
    color: var(--fg-muted);
    max-height: min(60vh, 34rem);
    overflow: auto;
  }

  /* A grouped run reads left to right: what the tool was, how many times,
     then what it was pointed at. */
  details.block summary .tool-name {
    color: var(--fg);
  }

  details.block summary .count {
    margin-left: var(--space-2);
    color: var(--accent);
    font-size: var(--text-sm);
  }

  details.block summary .targets {
    margin-left: var(--space-3);
    color: var(--fg-muted);
  }

  details.block summary .targets::before {
    content: "— ";
  }

  .elapsed {
    margin-left: var(--space-2);
    color: var(--fg-muted);
    font-size: var(--text-sm);
  }

  /* A call whose result hasn't come back yet — the row is still being
     written to. */
  .running-dot {
    vertical-align: middle;
    margin-left: var(--space-3);
  }

  details.call {
    border-top: 1px solid var(--border);
    padding: 0.35rem 0 0.3rem 0.5rem;
  }

  details.call summary {
    cursor: pointer;
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    color: var(--fg-muted);
    overflow-wrap: anywhere;
  }

  details.call[open] summary { margin-bottom: var(--space-2); }

  details.call pre {
    background: transparent;
    margin: 0;
    padding: 0;
    font-size: var(--text-sm);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    color: var(--fg);
    max-height: min(50vh, 28rem);
    overflow: auto;
  }

  /* An Edit, as the text it replaces over the text replacing it. Placed
     after the `details … pre` rules above, which it overrides. */
  details pre.edit-before,
  details pre.edit-after {
    padding: 0.3rem 0.5rem;
    border-left: 2px solid;
  }

  details pre.edit-before {
    background: var(--diff-del-bg);
    border-color: var(--diff-del-fg);
  }

  details pre.edit-after {
    margin-top: 2px;
    background: var(--diff-add-bg);
    border-color: var(--diff-add-fg);
  }

  .edit-note {
    font-size: var(--text-xs);
    color: var(--fg-muted);
    margin-bottom: var(--space-2);
  }

  /* The result sits under the call that asked for it, divided from the input
     rather than split into a card of its own. */
  .call-result {
    margin-top: var(--space-3);
    padding-top: 0.4rem;
    border-top: 1px dashed var(--border);
  }

  .call-result pre {
    color: var(--fg-muted);
  }

  /* A file's contents are the point of reading it, not a status line. */
  .call-result pre.read {
    color: var(--fg);
  }

  .system, .done {
    color: var(--fg-muted);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    padding: 0.15rem 0;
    overflow-wrap: anywhere;
    max-width: min(100%, var(--measure));
  }

  .done.err { color: var(--danger-text); }

  /* The app's own word in the transcript: a single line, like the rows around it. */
  .notice :global(p) { margin: 0; }

  .fail-banner {
    background: var(--danger-soft-bg);
    border: 1px solid var(--danger-soft-border);
    border-radius: var(--radius-md);
    padding: 0.55rem 0.85rem;
    font-size: var(--text-md);
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem 0.6rem;
    align-items: baseline;
  }

  .fail-banner .label {
    color: var(--danger-text);
    font-weight: var(--weight-semibold);
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .fail-banner .reason {
    color: var(--fg);
    font-family: var(--font-mono);
    font-size: var(--text-md);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    min-width: 0;
  }
</style>
