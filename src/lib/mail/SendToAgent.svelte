<script lang="ts">
  import { dismissOnMove, menuStyle, placeMenu, stepActive, type Placement } from "$lib/menus/menu";
  import { store } from "$lib/state/store.svelte";
  import { DEFAULT_ASK } from "./list";
  import { mail } from "./mail.svelte";
  import MailIcon from "./MailIcon.svelte";

  /**
   * "Send to Agent": Spawn an Agent in a Project the user picks, with the
   * selected conversation, quoted, as its Task. The user can say what it
   * should do, and see exactly what it will be handed before it starts.
   */
  let { trigger, onclose }: { trigger: HTMLElement; onclose: () => void } = $props();

  let panel: HTMLElement | undefined = $state();
  let noteEl: HTMLTextAreaElement | undefined = $state();
  let placement = $state<Placement | null>(null);

  const projects = $derived(store.projects);
  let picked = $state<string | null>(store.selectedProjectId ?? store.projects[0]?.id ?? null);
  let active = $state(Math.max(0, store.projects.findIndex((p) => p.id === picked)));
  let note = $state("");
  let preview = $state<string | null>(null);
  let busy = $state(false);

  $effect(() => {
    placement = placeMenu(trigger, { minWidth: 380, maxHeight: 520, align: "right" });
    noteEl?.focus();
    return dismissOnMove(() => [trigger, panel], onclose, { scroll: false });
  });

  async function showPreview() {
    if (preview !== null) {
      preview = null;
      return;
    }
    try {
      preview = await mail.taskFor(note);
    } catch (e) {
      mail.error = String(e);
    }
  }

  async function start() {
    if (!picked || busy) return;
    busy = true;
    try {
      if (await mail.sendToAgent(picked, note)) onclose();
    } catch (e) {
      mail.error = String(e);
    } finally {
      busy = false;
    }
  }

  function key(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onclose();
      trigger.focus();
      return;
    }
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      void start();
    }
  }

  function listKey(e: KeyboardEvent) {
    const next = stepActive(e.key, active, projects.length);
    if (next !== null) {
      e.preventDefault();
      active = next;
      picked = projects[next].id;
    }
  }
</script>

{#if placement}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="popover send"
    role="dialog"
    aria-label="Send to an agent"
    tabindex="-1"
    bind:this={panel}
    style={menuStyle(placement)}
    onkeydown={key}
  >
    <div class="head">
      <MailIcon name="agent" />
      <span>Hand this conversation to a new agent</span>
    </div>

    <label class="field-label" for="mail-agent-note">What should it do?</label>
    <textarea
      id="mail-agent-note"
      class="textarea"
      rows="2"
      bind:this={noteEl}
      bind:value={note}
      placeholder={DEFAULT_ASK}
    ></textarea>

    <div class="field-label">In which project?</div>
    {#if projects.length === 0}
      <p class="none">Add a project in Agents first.</p>
    {:else}
      <div class="projects" role="listbox" aria-label="Project" tabindex="0" onkeydown={listKey}>
        {#each projects as p, i (p.id)}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            class="menu-item"
            class:on={p.id === picked}
            role="option"
            tabindex="-1"
            aria-selected={p.id === picked}
            data-active={i === active}
            onpointermove={() => (active = i)}
            onclick={() => {
              picked = p.id;
              active = i;
            }}
          >
            <span class="menu-tick" aria-hidden="true">{p.id === picked ? "✓" : ""}</span>
            <span class="menu-label">{p.name}</span>
            <span class="menu-note">{p.path.split("/").slice(-2).join("/")}</span>
          </div>
        {/each}
      </div>
    {/if}

    {#if preview !== null}
      <pre class="preview">{preview}</pre>
    {/if}

    <p class="why">
      The mail goes in quoted, and the agent is told to treat it as information,
      not instructions. It runs on the model and mode you used last.
    </p>

    <div class="actions">
      <button class="btn btn-ghost btn-sm" onclick={showPreview}>
        {preview === null ? "Show what it gets" : "Hide"}
      </button>
      <span class="spacer"></span>
      <button class="btn btn-sm" onclick={onclose}>Cancel</button>
      <button class="btn btn-primary btn-sm" disabled={!picked || busy} onclick={start}>
        {busy ? "Starting…" : "Start agent"}
      </button>
    </div>
  </div>
{/if}

<style>
  .send {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-5);
    overflow-y: auto;
  }

  .head {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    font-weight: var(--weight-semibold);
    font-size: var(--text-lg);
    margin-bottom: var(--space-2);
  }

  .head :global(.icon) {
    color: var(--accent);
    width: 1.2rem;
    height: 1.2rem;
  }

  .textarea {
    resize: vertical;
    min-height: 3.2rem;
  }

  .projects {
    max-height: 11rem;
    overflow-y: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    padding: var(--menu-pad);
  }

  .projects:focus-visible {
    outline: var(--focus-outline);
    outline-offset: var(--focus-offset);
  }

  .none {
    margin: 0;
    color: var(--fg-muted);
    font-size: var(--text-sm);
  }

  .preview {
    margin: 0;
    max-height: 12rem;
    overflow: auto;
    padding: var(--space-4);
    background: var(--code-bg);
    border-radius: var(--radius-md);
    font-family: var(--font-mono);
    font-size: var(--text-2xs);
    white-space: pre-wrap;
  }

  .why {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--fg-muted);
    line-height: var(--leading-snug);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin-top: var(--space-2);
  }

  .spacer {
    flex: 1;
  }
</style>
