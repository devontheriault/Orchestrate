<script lang="ts">
  /**
   * The list that rises out of the composer when a prompt starts with `/`:
   * the commands `claude` would accept, narrowed by what's typed. Unlike the
   * pickers it never takes focus — the user is still typing, so the textarea
   * keeps the keyboard and walks this list for them (see AgentComposer), the
   * way a combobox does.
   */
  import type { SlashCommand } from "$lib/api";
  import { dismissOnMove, menuStyle, placeMenu, type Placement } from "$lib/menus/menu";
  import { blurb } from "./slash";

  let {
    id,
    anchor,
    commands,
    active,
    query,
    loading,
    error,
    onpick,
    onhover,
    onclose,
  }: {
    /** For the textarea's `aria-controls` and `aria-activedescendant`. */
    id: string;
    /** The input box: the menu sits on it, as wide as it is. */
    anchor: HTMLElement;
    commands: SlashCommand[];
    active: number;
    /** What's typed after the `/`, for saying so when nothing matches. */
    query: string;
    loading: boolean;
    error: string | null;
    onpick: (c: SlashCommand) => void;
    onhover: (i: number) => void;
    onclose: () => void;
  } = $props();

  let listEl: HTMLElement | undefined = $state();
  let placement = $state<Placement | null>(null);
  /**
   * The last move was the pointer's. Only a keyboard move scrolls the list: the
   * row under the pointer is already in view, and nudging it would slide the
   * list out from under the pointer.
   */
  let hovered = false;

  $effect(() => {
    placement = placeMenu(anchor, { minWidth: 280, maxHeight: 340, align: "left" });
  });

  // The box it sits on is the trigger: pressing there is still typing.
  $effect(() => dismissOnMove(() => [listEl, anchor], onclose, { scroll: false }));

  $effect(() => {
    active;
    if (hovered) {
      hovered = false;
      return;
    }
    listEl
      ?.querySelector<HTMLElement>('[data-active="true"]')
      ?.scrollIntoView({ block: "nearest" });
  });

  /** `plugin:` greyed, so the eye lands on the command's own name. */
  function split(name: string): [string, string] {
    const i = name.indexOf(":");
    return i < 0 ? ["", name] : [name.slice(0, i + 1), name.slice(i + 1)];
  }
</script>

{#if placement}
  <!-- Pressing a row mustn't take focus from the textarea: the user is
       mid-prompt, and the keys they type next belong there. -->
  <div
    class="popover slash"
    style={menuStyle(placement)}
    onpointerdown={(e) => e.preventDefault()}
    role="presentation"
  >
    {#if commands.length}
      <ul bind:this={listEl} {id} class="menu list" role="listbox" aria-label="Slash commands">
        {#each commands as c, i (c.name)}
          {@const [ns, own] = split(c.name)}
          <!-- The textarea owns the keyboard, as a combobox's input does. -->
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <li
            id={`${id}-${i}`}
            role="option"
            aria-selected={i === active}
            data-active={i === active}
            class="menu-item row"
            onclick={() => onpick(c)}
            onpointermove={() => {
              if (i === active) return;
              hovered = true;
              onhover(i);
            }}
          >
            <span class="name">
              <span class="slash-mark">/</span><span class="ns">{ns}</span>{own}
              {#if c.argument_hint}<span class="args">{c.argument_hint}</span>{/if}
            </span>
            <span class="desc" title={c.description}>{blurb(c)}</span>
          </li>
        {/each}
      </ul>
    {:else}
      <p class="empty">
        {#if loading}
          Asking Claude Code for its commands…
        {:else if error}
          {error}
        {:else}
          No command matches <code>/{query}</code> — Enter sends it as typed.
        {/if}
      </p>
    {/if}
    <!-- The keys, said once at the foot rather than learned by accident. -->
    <p class="keys" aria-hidden="true">
      <span><kbd>↑</kbd><kbd>↓</kbd> choose</span>
      <span><kbd>Tab</kbd> complete</span>
      <span><kbd>Esc</kbd> dismiss</span>
    </p>
  </div>
{/if}

<style>
  /* The list scrolls; the key hints at the foot stay put under it. */
  .slash {
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  /* Two columns shared by every row, so the descriptions line up and read
     as one column — the name column as wide as the longest name it can hold. */
  .list {
    flex: 1 1 auto;
    min-height: 0;
    display: grid;
    grid-template-columns: fit-content(55%) minmax(0, 1fr);
    align-content: start;
  }

  /* One line a command: its name, then what it does, cut off before it wraps.
     A list of ninety is scanned, not read, and one line a row keeps ten in
     view at once. */
  .row {
    display: grid;
    grid-column: 1 / -1;
    grid-template-columns: subgrid;
    column-gap: var(--space-6);
    min-width: 0;
  }

  .name {
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-weight: var(--weight-medium);
  }

  .slash-mark,
  .ns {
    color: var(--fg-muted);
    font-weight: normal;
  }

  .args {
    margin-left: var(--space-3);
    color: var(--fg-muted);
    font-weight: normal;
    font-size: var(--text-xs);
  }

  .desc {
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--fg-muted);
    font-size: var(--text-xs);
  }

  .empty {
    margin: 0;
    padding: 0.7rem 0.85rem;
    color: var(--fg-muted);
    font-size: var(--text-sm);
  }

  .keys {
    flex: none;
    margin: 0;
    display: flex;
    gap: var(--space-5);
    padding: 0.35rem 0.85rem;
    border-top: var(--border-width) solid var(--border);
    color: var(--fg-muted);
    font-size: var(--text-2xs);
  }

  kbd {
    font: inherit;
    margin-right: 0.2rem;
    padding: 0 0.25rem;
    border: var(--border-width) solid var(--border);
    border-radius: var(--radius-sm);
  }
</style>
