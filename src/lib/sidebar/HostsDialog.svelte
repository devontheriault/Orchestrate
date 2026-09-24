<script lang="ts">
  /**
   * The Hosts this window talks to: this machine's, and any other of the
   * user's machines added by its Tailscale name (ADR 0012). Adding one is all
   * it takes — that machine's Host lets this window in if it's the same
   * Tailscale user. Removing one only stops this window listening: the agents
   * there are that machine's, and carry on.
   */
  import { hosts } from "$lib/state/hosts.svelte";
  import { store } from "$lib/state/store.svelte";
  import type { HostStatus } from "$lib/api";

  let { onclose }: { onclose: () => void } = $props();

  let name = $state("");
  let adding = $state(false);
  let error = $state<string | null>(null);

  let inputEl: HTMLInputElement | undefined = $state();
  $effect(() => inputEl?.focus());

  async function add(e: SubmitEvent) {
    e.preventDefault();
    if (!name.trim() || adding) return;
    adding = true;
    error = await hosts.add(name);
    adding = false;
    if (!error) name = "";
  }

  async function remove(id: string) {
    error = await hosts.remove(id);
    // Its agents leave the window with it.
    if (!error) store.refresh();
  }

  function dot(s: HostStatus): string {
    switch (s.state) {
      case "connected":
        return "completed";
      case "connecting":
        return "stopped";
      case "updating":
        return "orphaned";
      default:
        return "failed";
    }
  }

  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onclose();
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
    if (e.target === e.currentTarget) onclose();
  }}
>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="hosts-title">
    <h2 id="hosts-title">Hosts</h2>
    <p class="lede">
      Each of your machines with this app runs a Host, which keeps its agents working whether or
      not a window is open. Add another machine to see and start agents on it from here.
    </p>

    <ul class="list">
      {#each hosts.list as h (h.id)}
        <li>
          <span class={`status-dot status-${dot(h.status)}`}></span>
          <span class="who">
            <span class="name">{hosts.label(h.id)}</span>
            <span class="state">
              {#if h.local}
                this machine
              {:else}
                {h.id}{#if hosts.problem(h.id)}&nbsp;· {hosts.problem(h.id)}{/if}
              {/if}
              {#if h.status.state === "refused"}— {h.status.reason}{/if}
            </span>
          </span>
          {#if !h.local}
            <button class="btn btn-ghost btn-sm" onclick={() => remove(h.id)}>Remove</button>
          {/if}
        </li>
      {/each}
    </ul>

    <form onsubmit={add}>
      <input
        bind:this={inputEl}
        bind:value={name}
        placeholder="Tailscale name, like desktop"
        aria-label="Tailscale name of the machine to add"
        spellcheck="false"
        autocomplete="off"
      />
      <button class="btn btn-primary" disabled={!name.trim() || adding}>
        {adding ? "Adding…" : "Add"}
      </button>
    </form>
    {#if error}<p class="error">{error}</p>{/if}
    <p class="hint">
      The machine needs this app installed, and Tailscale running and logged in as you. Only your
      own Tailscale user's machines are let in.
    </p>

    <footer>
      <button class="btn" onclick={onclose}>Done</button>
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

  .lede,
  .hint {
    margin: 0;
    font-size: var(--text-md);
    line-height: var(--leading-normal);
  }

  .hint {
    color: var(--fg-muted);
    font-size: var(--text-sm);
  }

  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--panel-bg);
  }

  .list li {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding: var(--space-3) var(--space-5);
  }

  .list li + li {
    border-top: 1px solid var(--border);
  }

  .who {
    display: flex;
    flex-direction: column;
    min-width: 0;
    flex: 1;
  }

  .name {
    font-size: var(--text-md);
    font-weight: var(--weight-semibold);
  }

  .state {
    font-size: var(--text-xs);
    color: var(--fg-muted);
    overflow-wrap: anywhere;
  }

  form {
    display: flex;
    gap: var(--space-3);
  }

  input {
    flex: 1;
    min-width: 0;
    font: inherit;
    font-size: var(--text-md);
    padding: 0.45rem 0.6rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--panel-bg);
    color: var(--fg);
  }

  input:focus {
    outline: none;
    border-color: var(--accent);
  }

  .error {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--danger-fg);
  }

  footer {
    display: flex;
    justify-content: flex-end;
  }
</style>
