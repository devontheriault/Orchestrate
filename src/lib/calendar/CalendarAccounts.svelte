<script lang="ts">
  /**
   * The calendar accounts on the Host, and connecting another: Google or
   * Microsoft, signed in through the browser, or any CalDAV server with a user
   * name and an app password. The provider keeps everything; removing an
   * account here only forgets it on the Host.
   *
   * Signing in needs an app registered with the provider. A build can come
   * with one (see docs/calendar-accounts.md); without it, the user registers
   * their own and pastes it here.
   *
   * A phone has no side panel, so here it also shows or hides each calendar.
   */
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api, type CalendarAccount } from "$lib/api";
  import Link from "$lib/markdown/Link.svelte";
  import { hosts } from "$lib/state/hosts.svelte";
  import { viewport } from "$lib/layout/viewport.svelte";
  import { calendar } from "./calendar.svelte";
  import CalendarList from "./CalendarList.svelte";
  import { ago } from "./layout";

  const DOCS = "https://github.com/devontheriault/DevCode/blob/main/docs/calendar-accounts.md";

  type Tab = "google" | "microsoft" | "caldav";
  const tabs: { id: Tab; label: string }[] = [
    { id: "google", label: "Google" },
    { id: "microsoft", label: "Outlook" },
    { id: "caldav", label: "CalDAV" },
  ];
  let tab = $state<Tab>(
    calendar.accounts === "caldav" || calendar.accounts === "microsoft" ? calendar.accounts : "google",
  );
  const provider = $derived(tab === "microsoft" ? "Microsoft" : "Google");

  const overview = $derived(calendar.overview);
  const hostName = $derived(hosts.label(calendar.host));

  // The app registered with Google or Microsoft, for a Host that has none
  // yet, or to replace the one it has.
  let clientId = $state("");
  let clientSecret = $state("");
  let changingClient = $state(false);
  let savingClient = $state(false);
  let clientError = $state<string | null>(null);
  const hasClient = $derived(
    tab === "microsoft" ? !!overview?.microsoft_client : !!overview?.google_client,
  );
  const needsClient = $derived(!hasClient || changingClient);
  $effect(() => {
    tab;
    changingClient = false;
    clientError = null;
  });

  async function saveClient(e: SubmitEvent) {
    e.preventDefault();
    savingClient = true;
    clientError = null;
    try {
      if (tab === "microsoft") await api.calendarSetMicrosoftClient(calendar.host, clientId);
      else await api.calendarSetGoogleClient(calendar.host, clientId, clientSecret);
      clientSecret = "";
      changingClient = false;
      await calendar.load();
    } catch (err) {
      clientError = String(err);
    } finally {
      savingClient = false;
    }
  }

  // CalDAV.
  let url = $state("");
  let username = $state("");
  let password = $state("");
  let adding = $state(false);
  let addError = $state<string | null>(null);

  async function addCaldav(e: SubmitEvent) {
    e.preventDefault();
    adding = true;
    addError = null;
    try {
      await api.calendarAddCaldav(calendar.host, url, username, password);
      url = username = password = "";
      await calendar.load();
    } catch (err) {
      addError = String(err);
    } finally {
      adding = false;
    }
  }

  /** The account a Remove was pressed on, waiting for its confirmation. */
  let removing = $state<string | null>(null);

  function remove(a: CalendarAccount) {
    if (removing !== a.id) {
      removing = a.id;
      setTimeout(() => {
        if (removing === a.id) removing = null;
      }, 4000);
      return;
    }
    removing = null;
    void calendar.removeAccount(a.id);
  }

  function statusLine(a: CalendarAccount): string {
    switch (a.status.state) {
      case "ok":
        return `Synced ${ago(new Date(a.status.at), calendar.now)}`;
      case "syncing":
        return "Syncing…";
      case "idle":
        return "Waiting to sync";
      default:
        return a.status.message;
    }
  }

  function close() {
    calendar.accounts = null;
  }

  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });
</script>

<div
  class="backdrop"
  role="presentation"
  onclick={(e) => {
    if (e.target === e.currentTarget) close();
  }}
>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="cal-accounts-title">
    <header>
      <h2 id="cal-accounts-title">{viewport.phone ? "Calendars" : "Calendar accounts"}</h2>
      <button class="btn btn-ghost btn-icon" aria-label="Close" onclick={close}>
        <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
          <path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" />
        </svg>
      </button>
    </header>

    {#if viewport.phone && overview?.accounts.length}
      <div class="shown"><CalendarList /></div>
    {/if}

    {#if overview?.accounts.length}
      <ul class="list">
        {#each overview.accounts as a (a.id)}
          <li>
            <span class="kind" data-kind={a.kind} aria-hidden="true">{a.kind === "google" ? "G" : a.kind === "microsoft" ? "M" : "@"}</span>
            <span class="who">
              <span class="name">{a.name}</span>
              <span
                class="state"
                class:bad={a.status.state === "error" || a.status.state === "reconnect"}
              >
                {statusLine(a)}
              </span>
            </span>
            {#if a.status.state === "reconnect" && a.kind !== "caldav"}
              {@const kind = a.kind}
              <button
                class="btn btn-sm"
                disabled={!calendar.onHostMachine || calendar.signingIn}
                onclick={() => calendar.signIn(kind, openUrl)}
              >
                Sign in again
              </button>
            {/if}
            <button class="btn btn-ghost btn-sm" class:confirm={removing === a.id} onclick={() => remove(a)}>
              {removing === a.id ? "Remove?" : "Remove"}
            </button>
          </li>
        {/each}
      </ul>
    {/if}

    <div class="add">
      <div class="tabs" role="tablist" aria-label="Add an account">
        {#each tabs as t (t.id)}
          <button role="tab" aria-selected={tab === t.id} class:on={tab === t.id} onclick={() => (tab = t.id)}>
            {t.label}
          </button>
        {/each}
      </div>

      {#if tab !== "caldav"}
        {#if !calendar.onHostMachine}
          <p class="note">
            Sign in from a window on {hostName}: {provider} sends your browser back to the machine
            that keeps your calendars, so the sign-in has to happen there.
          </p>
        {:else if needsClient}
          <form class="form" onsubmit={saveClient}>
            <p class="note">
              {#if tab === "microsoft"}
                This build has no Microsoft app to sign in as. Register one in Microsoft Entra
                (<Link href={DOCS}>the steps</Link>, about five minutes) and paste its
                application (client) ID here. It stays on {hostName}.
              {:else}
                This build has no Google OAuth client to sign in as. Make one in your Google Cloud
                project (<Link href={DOCS}>the steps</Link>, about five minutes) and paste its ID
                and secret here. They stay on {hostName}, readable only by you.
              {/if}
            </p>
            <label class="field-label" for="cal-client-id">
              {tab === "microsoft" ? "Application (client) ID" : "Client ID"}
            </label>
            <input
              id="cal-client-id"
              class="input input-mono"
              bind:value={clientId}
              placeholder={tab === "microsoft" ? "00000000-0000-0000-0000-000000000000" : "1234-abc.apps.googleusercontent.com"}
              spellcheck="false"
              autocomplete="off"
            />
            {#if tab === "google"}
              <label class="field-label" for="cal-client-secret">Client secret</label>
              <input
                id="cal-client-secret"
                class="input input-mono"
                type="password"
                bind:value={clientSecret}
                placeholder="GOCSPX-…"
                autocomplete="off"
              />
            {/if}
            {#if clientError}<p class="field-error">{clientError}</p>{/if}
            <div class="actions">
              {#if changingClient}
                <button type="button" class="btn btn-ghost" onclick={() => (changingClient = false)}>Cancel</button>
              {/if}
              <button
                class="btn btn-primary"
                disabled={!clientId.trim() || (tab === "google" && !clientSecret.trim()) || savingClient}
              >
                {savingClient ? "Saving…" : "Save"}
              </button>
            </div>
          </form>
        {:else}
          <div class="form">
            <p class="note">
              {#if tab === "microsoft"}
                Outlook.com, Hotmail, or a work or school Microsoft 365 account. Your browser opens on
                Microsoft's sign-in; the app asks to read and change your calendars, so you can make
                Calendar events and invite people from here.
              {:else}
                Your browser opens on Google's sign-in. The app asks to read your calendars and change
                Calendar events on them, so you can make them and invite people from here.
              {/if}
              Nothing is sent to anyone unless you do it here yourself.
            </p>
            {#if calendar.signingIn}
              <p class="waiting"><span class="spinner" aria-hidden="true"></span>Waiting for you to finish in your browser…</p>
            {/if}
            {#if calendar.signInError}<p class="field-error">{calendar.signInError}</p>{/if}
            <div class="actions">
              <button type="button" class="btn btn-ghost btn-sm" onclick={() => (changingClient = true)}>
                Use your own app
              </button>
              <button class="btn btn-primary" onclick={() => calendar.signIn(tab === "microsoft" ? "microsoft" : "google", openUrl)}>
                {calendar.signingIn ? "Open the sign-in again" : `Sign in with ${provider}`}
              </button>
            </div>
          </div>
        {/if}
      {:else}
        <form class="form" onsubmit={addCaldav}>
          <p class="note">
            Fastmail, iCloud, Nextcloud and most others. Use an app password where the provider
            offers one.
          </p>
          <label class="field-label" for="cal-url">Server</label>
          <input
            id="cal-url"
            class="input"
            bind:value={url}
            placeholder="https://caldav.fastmail.com"
            spellcheck="false"
            autocomplete="url"
          />
          <label class="field-label" for="cal-user">User name</label>
          <input id="cal-user" class="input" bind:value={username} spellcheck="false" autocomplete="username" />
          <label class="field-label" for="cal-pass">Password</label>
          <input id="cal-pass" class="input" type="password" bind:value={password} autocomplete="current-password" />
          {#if addError}<p class="field-error">{addError}</p>{/if}
          <div class="actions">
            <button class="btn btn-primary" disabled={!url.trim() || adding}>
              {adding ? "Connecting…" : "Add account"}
            </button>
          </div>
        </form>
      {/if}
    </div>
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
    background: var(--float-bg, var(--surface));
    -webkit-backdrop-filter: var(--float-filter, none);
    backdrop-filter: var(--float-filter, none);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    width: min(31rem, 100%);
    max-height: 100%;
    overflow-y: auto;
    box-shadow: var(--shadow-modal);
    padding: 1.1rem 1.25rem 1.1rem;
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  h2 {
    margin: 0;
    font-size: var(--text-2xl);
    font-weight: var(--weight-semibold);
  }

  /* The checks line up with the dialog's edge, not their rows'. */
  .shown {
    margin: 0 calc(-1 * var(--space-3));
  }

  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
  }

  .list li {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding: var(--space-4) var(--space-5);
  }

  .list li + li {
    border-top: 1px solid var(--border);
  }

  .kind {
    flex: none;
    width: 1.7rem;
    height: 1.7rem;
    border-radius: 50%;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: var(--hover);
    font-weight: var(--weight-bold);
    font-size: var(--text-sm);
    color: var(--fg-muted);
  }

  .kind[data-kind="google"],
  .kind[data-kind="microsoft"] {
    color: var(--accent);
  }

  .who {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .state {
    font-size: var(--text-xs);
    color: var(--fg-muted);
  }

  .state.bad {
    color: var(--danger-text);
  }

  .confirm {
    color: var(--danger-text);
  }

  .add {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .tabs {
    display: inline-flex;
    align-self: flex-start;
    padding: 2px;
    border-radius: var(--radius-md);
    background: var(--hover);
  }

  .tabs button {
    padding: 0.25rem 0.85rem;
    border: none;
    border-radius: calc(var(--radius-md) - 2px);
    background: none;
    color: var(--fg-muted);
    font: inherit;
    font-size: var(--text-sm);
    cursor: pointer;
  }

  .tabs button.on {
    background: var(--surface);
    color: var(--fg);
    box-shadow: var(--shadow-sm);
  }

  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .field-label {
    margin-top: var(--space-2);
  }

  .note {
    margin: 0;
    font-size: var(--text-sm);
    line-height: var(--leading-normal);
    color: var(--fg-muted);
  }

  .field-error {
    margin: 0;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: var(--space-3);
    margin-top: var(--space-3);
  }

  .actions :first-child:not(:last-child) {
    margin-right: auto;
  }

  .waiting {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    margin: 0;
    font-size: var(--text-sm);
  }

  .spinner {
    width: 0.85rem;
    height: 0.85rem;
    border-radius: 50%;
    border: 2px solid var(--border);
    border-top-color: var(--accent);
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
