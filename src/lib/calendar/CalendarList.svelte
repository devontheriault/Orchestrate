<script lang="ts">
  /**
   * Each account's calendars, each with a check to show or hide it: the
   * side panel's, and on a phone, which has no side panel, the accounts
   * dialog's.
   */
  import { calendar } from "./calendar.svelte";
  import { calendarKey } from "./layout";

  const accounts = $derived(calendar.overview?.accounts ?? []);
</script>

<div class="calendars">
  {#each accounts as a (a.id)}
    <section>
      <h3 title={a.name}>{a.name}</h3>
      <ul>
        {#each a.calendars as c (c.id)}
          {@const on = calendar.isShown(a.id, c.id)}
          <li>
            <label class="cal" style:--cal={c.color}>
              <input
                type="checkbox"
                checked={on}
                onchange={() => calendar.toggle(a.id, c.id)}
                aria-label={`Show ${c.name}`}
              />
              <span class="check" aria-hidden="true">
                <svg viewBox="0 0 12 12" width="9" height="9"><path d="M2.5 6.2l2.2 2.2 4.8-4.8" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" /></svg>
              </span>
              <span class="cal-name" data-key={calendarKey(a.id, c.id)}>{c.name}</span>
            </label>
          </li>
        {/each}
      </ul>
    </section>
  {/each}
</div>

<style>
  .calendars {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
  }

  h3 {
    margin: 0 0 var(--space-3) var(--space-3);
    font-size: var(--text-2xs);
    font-weight: var(--weight-semibold);
    color: var(--fg-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .cal {
    --c: var(--cal, var(--accent));
    position: relative;
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding: 0.3rem var(--space-3);
    border-radius: var(--radius-md);
    font-size: var(--text-sm);
    cursor: pointer;
  }

  .cal:hover {
    background: var(--hover);
  }

  /* A finger's row on a phone. */
  :global(html[data-frame="mobile"]) .cal {
    min-height: 2.75rem;
    font-size: var(--text-md);
  }

  .cal input {
    position: absolute;
    opacity: 0;
    pointer-events: none;
  }

  .check {
    flex: none;
    width: 0.95rem;
    height: 0.95rem;
    border-radius: var(--radius-xs);
    border: 1.5px solid var(--c);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: transparent;
    transition: background var(--transition-fast);
  }

  .cal input:checked + .check {
    background: var(--c);
    color: var(--surface);
  }

  .cal input:focus-visible + .check {
    box-shadow: var(--focus-ring);
  }

  .cal-name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
