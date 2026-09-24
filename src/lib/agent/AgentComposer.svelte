<script module lang="ts">
  /**
   * What the user was typing and picking, set aside while they look elsewhere.
   * Module-level, so it outlives the composer: it unmounts whenever the Diff
   * tab is open, and coming back shouldn't cost the user their words.
   */
  type Draft = {
    prompt: string;
    attachments: string[];
    model: string;
    effort: string;
    mode: string;
  };

  /**
   * Unsent drafts, by agent id — or `new:<project id>` for the opening prompt
   * of an agent not spawned yet. Only boxes with something in them are held.
   */
  const drafts = new Map<string, Draft>();
</script>

<script lang="ts">
  import { untrack } from "svelte";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { open } from "@tauri-apps/plugin-dialog";
  import { store } from "$lib/state/store.svelte";
  import { slash } from "$lib/state/slash.svelte";
  import { api, type AgentOptions } from "$lib/api";
  import { models } from "$lib/state/models.svelte";
  import { theme } from "$lib/theme/theme.svelte";
  import { tagColor } from "$lib/theme/tags";
  import { usage as usageWindow } from "$lib/usage/usage.svelte";
  import { advisorLabel, DEFAULT_EFFORT, DEFAULT_MODE, DEFAULT_MODEL } from "$lib/picks";
  import { stepActive } from "$lib/menus/menu";
  import Attachments from "./Attachments.svelte";
  import ModelPicker from "$lib/menus/ModelPicker.svelte";
  import ModePicker from "$lib/menus/ModePicker.svelte";
  import OptionsPicker from "$lib/menus/OptionsPicker.svelte";
  import AgentQueue from "./AgentQueue.svelte";
  import TurnStats from "./TurnStats.svelte";
  import SlashMenu from "./SlashMenu.svelte";
  import { completeCommand, matchCommands, namesCommand, slashQuery, typedCommand } from "./slash";
  import { interpret, menuCommands, mergeTarget, type Action } from "./appCommands";

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
  /** Files going out with the prompt, by absolute path. */
  let attachments = $state<string[]>([]);
  /** Files are being dragged over the window, and would land here. */
  let dropping = $state(false);
  /** The picker's value; DEFAULT_MODEL passes no --model. */
  let model = $state(DEFAULT_MODEL);
  /** The effort chosen beside it; DEFAULT_EFFORT passes no --effort. */
  let effort = $state(DEFAULT_EFFORT);
  /** What this turn may do without asking: YOLO, or plan and write nothing. */
  let mode = $state(DEFAULT_MODE);
  let textarea: HTMLTextAreaElement | undefined = $state();
  let box: HTMLElement | undefined = $state();

  const uid = $props.id();
  /** Where the caret sits in the prompt: the `/` menu completes the word it's in. */
  let caret = $state(0);
  /** The prompt as it stood when Esc put the `/` menu away, until it changes. */
  let dismissed = $state<string | null>(null);
  /** The row the keyboard is on in the `/` menu. */
  let active = $state(0);

  /**
   * Where `claude` would run this prompt, which decides the commands it takes:
   * a Project's own commands and skills live in its tree.
   */
  const commandsDir = $derived(
    agent
      ? agent.worktree_path
      : store.projects.find((p) => p.id === store.selectedProjectId)?.path,
  );
  const commands = $derived(slash.for(commandsDir));
  /** The menu's list: the app's own commands, and what `claude` offers less the rest. */
  const offered = $derived(menuCommands(commands.list));
  /** What's typed of a command at the head of the prompt, or null. */
  const query = $derived(slashQuery(prompt, caret));
  const matches = $derived(query === null ? [] : matchCommands(offered, query));
  const menuOpen = $derived(query !== null && !inFlight && dismissed !== prompt);
  /** The command being given arguments, so what it takes can be shown. */
  const usage = $derived(typedCommand(offered, prompt));

  /**
   * What the last command the app answered did, or why it didn't. Said under
   * the box, where the eye is, until the user types again or it has had time
   * to be read.
   */
  let notice = $state<{ text: string; failed: boolean } | null>(null);
  let noticeTimer: ReturnType<typeof setTimeout> | undefined;

  function say(text: string, failed = false) {
    notice = { text, failed };
    clearTimeout(noticeTimer);
    noticeTimer = setTimeout(() => (notice = null), 8000);
  }

  $effect(() => () => clearTimeout(noticeTimer));

  /** The agent's options, or on a blank page the ones it will spawn with. */
  const options = $derived<AgentOptions>(agent ? agent.options ?? {} : store.prefs.options);

  /** Change some of the options: the agent's straight away, or the new agent's. */
  async function setOptions(change: AgentOptions): Promise<boolean> {
    const next = { ...options, ...change };
    if (agent) return store.setAgentOptions(agent.id, next);
    store.prefs.rememberOptions(next);
    return true;
  }

  // Ask afresh each time the menu comes up: skills get added mid-session, and
  // the list shown meanwhile is the last one, so reopening never blanks it.
  $effect(() => {
    if (menuOpen && commandsDir) {
      const dir = commandsDir;
      untrack(() => slash.load(dir));
    }
  });

  // A narrower list starts back at its best match.
  $effect(() => {
    query;
    active = 0;
  });

  function trackCaret() {
    if (textarea) caret = textarea.selectionStart;
  }

  function onInput() {
    trackCaret();
    notice = null;
  }

  /** Put `/name ` at the head of the prompt, ready for its arguments. */
  function pickCommand(name: string) {
    const done = completeCommand(prompt, name);
    prompt = done.text;
    caret = done.caret;
    textarea?.focus();
    requestAnimationFrame(() => textarea?.setSelectionRange(done.caret, done.caret));
  }

  // Grow with the text, up to a ceiling — a long follow-up shouldn't need
  // scrolling, but it shouldn't swallow the transcript either. scrollHeight is
  // rounded to a whole pixel while the lines themselves are fractional, so a
  // few pixels of slack keeps the last line from being clipped. Once it's
  // scrolling, typing at the end keeps the line being typed on exactly where it
  // sat before the ceiling — the browser alone would scroll it only as far as
  // the text, leaving it jammed against the bottom edge. The box holds its
  // height while the textarea collapses to be measured: WebKit clamps the
  // transcript's scroll to the momentarily taller pane, so every keystroke
  // would otherwise nudge a transcript sitting at the bottom up off it.
  function fit() {
    if (!textarea) return;
    if (box) box.style.height = `${box.offsetHeight}px`;
    textarea.style.height = "auto";
    textarea.style.height = `${textarea.scrollHeight + 4}px`;
    if (box) box.style.height = "";
    if (textarea.selectionEnd === textarea.value.length) {
      textarea.scrollTop = textarea.scrollHeight;
    }
  }

  $effect(() => {
    prompt;
    fit();
  });

  /** Which conversation the box is holding a message for. */
  const draftKey = $derived(
    agent ? agent.id : drafting ? `new:${store.selectedProjectId}` : null,
  );

  // A different agent — or a blank page for a new one — is a different
  // conversation: set the box's contents aside under the one being left, and
  // bring back whatever was left in the one arrived at. With nothing left
  // there, the picker starts on whatever the agent last ran on, or on the
  // user's habitual model for a new agent — untracked, so a record updating
  // mid-edit doesn't undo a model just picked.
  $effect(() => {
    const key = draftKey;
    untrack(() => restore(key));
    return () => untrack(() => stash(key));
  });

  function stash(key: string | null) {
    if (!key) return;
    if (prompt.trim() || attachments.length) {
      drafts.set(key, { prompt, attachments, model, effort, mode });
    } else {
      drafts.delete(key);
    }
  }

  function restore(key: string | null) {
    // What a command did was said about the conversation being left.
    notice = null;
    const draft = key ? drafts.get(key) : undefined;
    if (draft) {
      ({ prompt, attachments, model, effort, mode } = draft);
      return;
    }
    prompt = "";
    attachments = [];
    const fresh = store.drafting && !store.selectedAgent;
    model = fresh
      ? store.prefs.spawnModel
      : store.selectedAgent?.model ?? DEFAULT_MODEL;
    effort = fresh
      ? store.prefs.spawnEffort
      : store.selectedAgent?.effort ?? DEFAULT_EFFORT;
    mode = fresh
      ? store.prefs.spawnMode
      : store.selectedAgent?.permission_mode ?? DEFAULT_MODE;
  }

  // The blank page exists to be typed into, so put the cursor there.
  $effect(() => {
    if (drafting) textarea?.focus();
  });

  async function send() {
    if (!prompt.trim() || inFlight) return;
    // A command the app answers goes nowhere near `claude`, so it works mid-Turn
    // too. A refusal leaves the text, to be corrected rather than retyped.
    const action = interpret(prompt, { models: models.list, styles: commands.styles });
    if (action) {
      if (action.do !== "refuse") prompt = "";
      await apply(action);
      return;
    }
    // Mid-Turn, the same gesture lines the message up instead: one `claude` per
    // worktree, so it goes out as its own Turn once this one ends.
    if (working) {
      if (queueing && store.queue.enqueue(prompt, attachments, model, effort, mode)) clear();
      return;
    }
    // Keep the text and files on failure either way, so the user can retry
    // rather than gather them again.
    const key = draftKey;
    const sent = drafting
      ? await store.spawn(prompt, attachments, model, effort, mode)
      : await store.resume(prompt, attachments, model, effort, mode);
    if (!sent) return;
    // The box may have moved on while it went out — a spawn opens the agent it
    // started — and so set what was sent aside as a draft. Drop that, and
    // leave whatever the box holds now alone.
    if (draftKey === key) clear();
    else if (key) drafts.delete(key);
  }

  function clear() {
    prompt = "";
    attachments = [];
  }

  /**
   * Do what a command the app answers asks. Model and effort move the pickers,
   * so they go out with the next prompt as a click on them would; the rest
   * change the agent, or the app, straight away.
   */
  async function apply(action: Action) {
    switch (action.do) {
      case "refuse":
        return say(action.why, true);
      case "model":
        model = action.model;
        return say(`Model: ${models.name(action.model)}, from the next prompt.`);
      case "effort":
        effort = action.effort;
        return say(`Effort: ${action.effort || "default"}, from the next prompt.`);
      case "advisor":
        if (await setOptions({ advisor: action.advisor }))
          say(`Advisor: ${action.advisor ? advisorLabel(action.advisor) : "default"}, from the next prompt.`);
        return;
      case "style":
        if (await setOptions({ output_style: action.style }))
          say(`Output style: ${action.style ?? "default"}, from the next prompt.`);
        return;
      case "theme":
        if (!action.theme) return theme.openPicker();
        theme.set(action.theme);
        return say(`Theme: ${theme.label}.`);
      case "usage":
        return usageWindow.show();
      case "color":
        if (!agent) return say("Spawn the agent first, then give it a colour.", true);
        if (await store.setAgentColor(agent.id, action.color))
          say(action.color ? `Tagged ${action.color}.` : "Colour cleared.");
        return;
      case "rename":
        if (!agent) return say("Spawn the agent first, then name it.", true);
        if (await store.renameAgent(agent.id, action.name))
          say(action.name ? `Renamed to “${action.name}”.` : "Back to Claude's name for it.");
        return;
      case "commit":
        return commit(action.message);
      case "merge":
        return merge(action.branch);
    }
  }

  const review = store.review;

  /** `/commit`: the Diff tab's Commit, under the agent's name unless told otherwise. */
  async function commit(message: string | null) {
    if (!agent) return say("Spawn the agent first, then commit its work.", true);
    if (working) return say("The agent is still writing. Commit once it has finished.", true);
    if (review.committing) return;
    const subject = message ?? review.suggestedMessage;
    if (!subject) return say("Give the commit a message: /commit <message>.", true);
    const made = await review.commit(subject);
    if (made) say(`Committed ${made.sha}: ${made.subject}`);
    else say(review.error ?? "The commit didn't go through.", true);
  }

  /**
   * `/merge`: the Diff tab's Merge. The diff and branches are read afresh
   * first — the tab may not have been opened since the worktree last moved.
   */
  async function merge(branch: string | null) {
    if (!agent) return say("Spawn the agent first, then merge its work.", true);
    if (working) return say("The agent is still writing. Merge once it has finished.", true);
    if (review.merging) return;
    const id = agent.id;
    await Promise.all([review.load(), review.loadBranches()]);
    if (store.selectedAgentId !== id) return;
    if (!review.diff) return say(review.error ?? "Couldn't read the agent's work.", true);
    if (review.diff.uncommitted) return say("Commit first: /commit, then /merge.", true);
    if (!review.diff.commits.length) return say("There's nothing committed to merge yet.", true);
    const pick = mergeTarget(branch, {
      targets: review.targets,
      mergedInto: review.mergedInto,
      fallback: review.defaultTarget,
    });
    if ("why" in pick) return say(pick.why, true);
    if (await review.merge(pick.branch)) return say(`Merged into ${pick.branch}.`);
    // A conflict is shown where it can be handed to a resolver.
    if (review.conflicts[id]) return store.showTab("diff");
    say(review.error ?? "The merge didn't go through.", true);
  }

  /** The box is up and taking input — the only time a file has somewhere to go. */
  const accepting = $derived((drafting || store.canContinue || working) && !inFlight);

  function attach(paths: string[]) {
    const fresh = paths.filter((p) => !attachments.includes(p));
    if (fresh.length) attachments = [...attachments, ...fresh];
    textarea?.focus();
  }

  function detach(path: string) {
    attachments = attachments.filter((p) => p !== path);
  }

  async function pick() {
    const picked = await open({ multiple: true, title: "Attach files" });
    if (picked) attach(picked);
  }

  // A pasted screenshot has no path for `claude` to read it from, so it's
  // written to disk first. Only when the clipboard holds no text: copying from
  // a document can carry an image rendition of the text too, and the text is
  // what was meant. WebKitGTK never gives the page a clipboard image, only
  // copied files' paths, so with no files either the backend looks for one.
  async function onPaste(e: ClipboardEvent) {
    const data = e.clipboardData;
    if (data?.getData("text/plain")) return;
    const files = Array.from(data?.files ?? []);
    e.preventDefault();
    try {
      if (!files.length) {
        const saved = await api.saveClipboardImage(pastedName("", "image/png"));
        if (saved) attach([saved]);
        return;
      }
      const saved = await Promise.all(
        files.map(async (f) =>
          api.saveAttachment(
            pastedName(f.name, f.type),
            new Uint8Array(await f.arrayBuffer()),
          ),
        ),
      );
      attach(saved);
    } catch (err) {
      store.error = String(err);
    }
  }

  /**
   * Clipboard images come named "image.png" or not at all, which tells the
   * user nothing once there are two of them. Name those after the moment
   * they were pasted instead.
   */
  function pastedName(name: string, type: string): string {
    if (name && !/^image\.\w+$/.test(name)) return name;
    const ext = type.split("/")[1]?.replace(/\W.*$/, "") || "png";
    const t = new Date();
    const pad = (n: number) => String(n).padStart(2, "0");
    return `pasted-${pad(t.getHours())}${pad(t.getMinutes())}${pad(t.getSeconds())}.${ext}`;
  }

  // Files dropped anywhere on the window land here: the composer is the one
  // place a file can go, so there's no target to aim for. The webview hands
  // over real paths only through Tauri's own drag-drop events — the DOM's
  // drop event has no paths to give.
  $effect(() => {
    if (!accepting) return;
    let unlisten: (() => void) | undefined;
    let gone = false;
    getCurrentWebview()
      .onDragDropEvent(({ payload }) => {
        if (payload.type === "enter") dropping = payload.paths.length > 0;
        else if (payload.type === "leave") dropping = false;
        else if (payload.type === "drop") {
          dropping = false;
          attach(payload.paths);
        }
      })
      .then((fn) => {
        if (gone) fn();
        else unlisten = fn;
      });
    return () => {
      gone = true;
      dropping = false;
      unlisten?.();
    };
  });

  /**
   * The `/` menu's keys, while it's up. The textarea keeps the keyboard, so it
   * walks the menu itself. Returns whether the key was the menu's.
   */
  function menuKey(e: KeyboardEvent): boolean {
    if (!menuOpen || e.isComposing) return false;
    if ((e.key === "ArrowDown" || e.key === "ArrowUp") && matches.length) {
      active = stepActive(e.key, active, matches.length) ?? active;
      return true;
    }
    if (e.key === "Escape") {
      dismissed = prompt;
      return true;
    }
    if (e.key === "Tab" && !e.shiftKey && matches.length) {
      pickCommand(matches[active].name);
      return true;
    }
    // Enter completes what's highlighted — unless the command is already typed
    // in full, when it goes out as it is: `/compact` then Enter just compacts.
    if (e.key === "Enter" && !e.shiftKey && !e.altKey && matches.length) {
      if (namesCommand(offered, query ?? "")) return false;
      pickCommand(matches[active].name);
      return true;
    }
    return false;
  }

  function onKeydown(e: KeyboardEvent) {
    if (menuKey(e)) {
      e.preventDefault();
      return;
    }
    // Esc on an untouched blank page walks back out of it.
    if (
      e.key === "Escape" &&
      drafting &&
      !prompt.trim() &&
      !attachments.length &&
      !inFlight
    ) {
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
      <!-- An agent tagged with a colour wears it on its box, as Claude Code's
           `/color` tints its prompt bar. -->
      <div
        class="box"
        class:dropping
        class:tagged={!!tagColor(agent?.color)}
        style:--tag={tagColor(agent?.color)}
        bind:this={box}
      >
        {#if attachments.length}
          <div class="files">
            <Attachments paths={attachments} onremove={inFlight ? undefined : detach} />
          </div>
        {/if}
        <textarea
          bind:this={textarea}
          bind:value={prompt}
          onkeydown={onKeydown}
          onkeyup={trackCaret}
          oninput={onInput}
          onclick={trackCaret}
          onpaste={onPaste}
          rows="1"
          role="combobox"
          aria-expanded={menuOpen}
          aria-controls={`${uid}-slash`}
          aria-autocomplete="list"
          aria-activedescendant={menuOpen && matches.length ? `${uid}-slash-${active}` : undefined}
          disabled={inFlight}
          placeholder={attachments.length && !prompt
            ? "Say what to do with the attached files…"
            : drafting
              ? "What should the agent do? Type / for commands"
              : "Reply to this agent, or type / for commands"}
        ></textarea>
        {#if menuOpen && box}
          <SlashMenu
            id={`${uid}-slash`}
            anchor={box}
            commands={matches}
            {active}
            query={query ?? ""}
            loading={commands.loading}
            error={commands.error}
            onpick={(c) => pickCommand(c.name)}
            onhover={(i) => (active = i)}
            onclose={() => (dismissed = prompt)}
          />
        {/if}
        {#if dropping}
          <!-- Over the whole box, so while a drag is on the window the eye is
               told where the files will land. -->
          <div class="drop" aria-hidden="true">Drop to attach</div>
        {/if}
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
                <rect x="3.5" y="3.5" width="9" height="9" rx="1.8" fill="currentColor" />
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
                  d="M8 12.75V3.25M8 3.25L3.5 7.75M8 3.25L12.5 7.75"
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
        <!-- Bottom-left, mirroring send: what goes out with the message on
             one side, the button that sends it on the other. After send in
             the markup, so Tab from the box reaches send first. -->
        <div class="slot left">
          <button
            class="clip"
            onclick={pick}
            disabled={inFlight}
            aria-label="Attach files"
            title="Attach files — or drop them on the window, or paste an image"
          >
            <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
              <path
                d="M13.2 7.6l-5 5a3.2 3.2 0 01-4.5-4.5l5.3-5.3a2.1 2.1 0 013 3L6.8 11a1 1 0 01-1.5-1.5l4.6-4.6"
                fill="none"
                stroke="currentColor"
                stroke-width="1.5"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
          </button>
        </div>
      </div>
      <div class="below">
        <span class="hint">
          {#if store.spawning}
            Spawning…
          {:else if store.sending}
            Sending…
          {:else if notice}
            <span class:failed={notice.failed} role="status">{notice.text}</span>
          {:else if usage?.argument_hint}
            <!-- What the command at the head of the prompt takes, while its
                 arguments are being typed. -->
            /{usage.name} {usage.argument_hint}
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
          <!-- Last: what it picks is the agent's, not just this prompt's. -->
          <OptionsPicker
            {options}
            styles={commands.styles}
            onchange={setOptions}
            onopen={() => commandsDir && slash.load(commandsDir)}
            disabled={inFlight}
            label={drafting ? "Options for the new agent" : "Options for this agent"}
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
    flex-direction: column;
    border: 1px solid var(--border);
    border-radius: var(--radius-xl);
    background: var(--panel-bg);
    transition: border-color var(--transition-fast);
  }

  .box:focus-within {
    border-color: var(--accent);
  }

  /* Tinted at rest and in full while typed in, so the tag reads either way. */
  .box.tagged {
    border-color: color-mix(in srgb, var(--tag) 45%, var(--border));
  }

  .box.tagged:focus-within {
    border-color: var(--tag);
  }

  .box.dropping {
    border-color: var(--accent);
    border-style: dashed;
  }

  /* A long list of files wraps into rows; past a few, it scrolls rather than
     pushing the textarea off the pane. */
  .files {
    padding: 0.6rem 0.7rem 0;
    max-height: calc(0.15 * var(--pane-h, 100vh));
    overflow-y: auto;
  }

  .drop {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: inherit;
    background: color-mix(in srgb, var(--accent) 10%, var(--panel-bg));
    color: var(--accent);
    font-weight: var(--weight-medium);
    pointer-events: none;
    /* Over the attach and send buttons too: mid-drag, neither is the point. */
    z-index: 1;
  }

  /* No flex-grow here: as a flex item with flex: 1 the textarea sizes to its
     one row and ignores the height fit() sets. It grows by a line per line,
     up to 16 lines or 30% of the pane, whichever comes first, then scrolls.
     The box grows upward because it sits at the foot of the pane, so the line
     being typed on stays where it was. */
  textarea {
    min-width: 0;
    resize: none;
    max-height: min(
      calc(0.3 * var(--pane-h, 100vh)),
      calc(16 * var(--text-lg) * var(--leading-normal) + 1.9rem)
    );
    overflow-y: auto;
    /* Side padding clears the attach and send buttons so text never runs
       under either. The bottom gives back the 4px of slack fit() adds under
       the text, so one line sits dead centre in the box. */
    padding: 0.95rem 3.25rem calc(0.95rem - 4px) 2.85rem;
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
     calls for stays put as the text grows. Raised so the button's middle
     lines up with the last line of text — on a single line, the box's middle. */
  .slot {
    position: absolute;
    right: 0.7rem;
    bottom: calc(0.95rem + var(--text-lg) * var(--leading-normal) / 2 - 1.85rem / 2);
    display: flex;
    align-items: center;
  }

  .slot.left {
    right: auto;
    left: 0.7rem;
  }

  /* Quiet beside send: an extra, not the thing the box is for. */
  .clip {
    width: 1.85rem;
    height: 1.85rem;
    padding: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: var(--radius-pill);
    background: none;
    color: var(--fg-muted);
    cursor: pointer;
    transition:
      background var(--transition-fast),
      color var(--transition-fast);
  }

  .clip:hover:not(:disabled) {
    background: var(--hover);
    color: var(--fg);
  }

  .clip:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .clip:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }

  .send {
    width: 1.85rem;
    height: 1.85rem;
    padding: 0;
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

  /* The hint gives way to the pickers, never the other way round: a notice
     long enough to wrap wraps in its own space rather than squeezing them. */
  .hint {
    flex: 1 1 0;
    min-width: 0;
    font-size: var(--text-2xs);
    color: var(--fg-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* A reason to hold up rather than a hint, and it may run long: it wraps. */
  .hint .failed {
    color: var(--danger-text);
    white-space: normal;
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
