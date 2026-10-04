<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { hosts } from "$lib/state/hosts.svelte";
  import { PROVIDERS, providerFor } from "./list";
  import { mail } from "./mail.svelte";
  import MailIcon from "./MailIcon.svelte";

  /** Connecting the one account a Host reads mail for. */
  let how = $state<"password" | "google">("password");

  let email = $state("");
  let password = $state("");
  let provider = $state<string>("gmail");
  let chosen = $state(false);
  let server = $state("");
  let port = $state(993);
  let plain = $state(false);
  let username = $state("");

  let clientId = $state("");
  let clientSecret = $state("");
  let showSteps = $state(false);

  let busy = $state(false);
  let failed = $state<string | null>(null);
  let browserUrl = $state<string | null>(null);

  // The address says where it's hosted, until the user picks for themselves.
  $effect(() => {
    if (chosen) return;
    provider = providerFor(email)?.id ?? (email.includes("@") ? "other" : provider);
  });

  const known = $derived(PROVIDERS.find((p) => p.id === provider) ?? null);
  /** The machine that will hold the account: the Host's own name for itself. */
  const machine = $derived(hosts.label(hosts.home));

  async function connect(e: SubmitEvent) {
    e.preventDefault();
    busy = true;
    failed = null;
    try {
      if (how === "google") {
        const { url } = await mail.setUp({ auth: "google", email, clientId, clientSecret });
        if (url) {
          browserUrl = url;
          await openUrl(url);
        }
      } else {
        await mail.setUp({
          auth: "password",
          email,
          password,
          server: known?.server ?? server,
          port: known?.port ?? port,
          security: !known && plain ? "plain" : "tls",
          username: !known && username ? username : undefined,
        });
      }
    } catch (err) {
      failed = String(err);
    } finally {
      busy = false;
    }
  }

  const waiting = $derived(mail.status?.state === "signing_in");
  const googleError = $derived(how === "google" && !waiting ? mail.status?.error : null);
</script>

<div class="setup">
  <form class="card" onsubmit={connect}>
    <div class="mark"><MailIcon name="mail" size="1.6rem" /></div>
    <h1>Connect your mail</h1>
    <p class="lede">
      Your mail stays on your provider's server. <b>{machine}</b> reads it over IMAP,
      keeps only a cache, and never sends anything.
    </p>

    <div class="tabs" role="tablist">
      <button
        type="button"
        role="tab"
        aria-selected={how === "password"}
        class:on={how === "password"}
        onclick={() => (how = "password")}
      >
        App password
      </button>
      <button
        type="button"
        role="tab"
        aria-selected={how === "google"}
        class:on={how === "google"}
        onclick={() => (how = "google")}
      >
        Sign in with Google
      </button>
    </div>

    {#if waiting}
      <div class="waiting">
        <span class="spinner"></span>
        <div>
          <strong>Finish signing in in your browser.</strong>
          <p>
            Google sends you back here when you're done.
            {#if browserUrl}
              <!-- svelte-ignore a11y_invalid_attribute -->
              <a href="#" onclick={(e) => { e.preventDefault(); void openUrl(browserUrl!); }}>Open the page again</a>.
            {/if}
          </p>
        </div>
      </div>
    {:else if how === "password"}
      <label class="field">
        <span class="field-label">Email address</span>
        <input class="input" type="email" bind:value={email} autocomplete="email" required placeholder="you@example.com" />
      </label>

      <div class="field">
        <span class="field-label">Provider</span>
        <div class="providers">
          {#each [...PROVIDERS, { id: "other", label: "Other" }] as p (p.id)}
            <button
              type="button"
              class="chip"
              class:on={provider === p.id}
              onclick={() => {
                provider = p.id;
                chosen = true;
              }}
            >
              {p.label}
            </button>
          {/each}
        </div>
      </div>

      {#if !known}
        <div class="row">
          <label class="field grow">
            <span class="field-label">IMAP server</span>
            <input class="input input-mono" bind:value={server} required placeholder="imap.example.com" spellcheck="false" />
          </label>
          <label class="field port">
            <span class="field-label">Port</span>
            <input class="input input-mono" type="number" bind:value={port} min="1" max="65535" required />
          </label>
        </div>
        <label class="field">
          <span class="field-label">Username, if it isn't the address</span>
          <input class="input" bind:value={username} autocomplete="username" spellcheck="false" />
        </label>
        <label class="check">
          <input type="checkbox" bind:checked={plain} />
          <span>No encryption — only for a test server on this computer</span>
        </label>
      {/if}

      <label class="field">
        <span class="field-label">{known ? "App password" : "Password"}</span>
        <input class="input" type="password" bind:value={password} autocomplete="current-password" required />
        {#if known}
          <span class="field-hint">{known.help} Your normal password won't work over IMAP.</span>
        {/if}
      </label>
    {:else}
      <p class="note">
        Gmail lets an app read mail with Google sign-in only through an OAuth client
        of your own. It's free and takes a few minutes, once.
        <button type="button" class="linkish" onclick={() => (showSteps = !showSteps)}>
          {showSteps ? "Hide the steps" : "Show me how"}
        </button>
      </p>
      {#if showSteps}
        <ol class="steps">
          <li>At <b>console.cloud.google.com</b>, make a project and enable the <b>Gmail API</b>.</li>
          <li>Under <b>Google Auth Platform → Audience</b>, choose External and add your address as a test user. Then <b>Publish</b> it, or Google signs you out every 7 days.</li>
          <li>Under <b>Data Access</b>, add the scope <code>https://mail.google.com/</code>.</li>
          <li>Under <b>Clients</b>, create a client of type <b>Desktop app</b>, and paste its ID and secret here.</li>
          <li>When Google warns the app isn't verified, it's your own: choose <b>Advanced → Go to …</b>.</li>
        </ol>
      {/if}
      <label class="field">
        <span class="field-label">Gmail address</span>
        <input class="input" type="email" bind:value={email} autocomplete="email" placeholder="you@gmail.com" />
      </label>
      <label class="field">
        <span class="field-label">OAuth client ID</span>
        <input class="input input-mono" bind:value={clientId} required spellcheck="false" placeholder="….apps.googleusercontent.com" />
      </label>
      <label class="field">
        <span class="field-label">Client secret</span>
        <input class="input input-mono" type="password" bind:value={clientSecret} required spellcheck="false" />
      </label>
    {/if}

    {#if failed || googleError}
      <p class="field-error" role="alert">{failed ?? googleError}</p>
    {/if}

    {#if !waiting}
      <button class="btn btn-primary btn-lg submit" type="submit" disabled={busy}>
        {#if busy}
          Connecting…
        {:else if how === "google"}
          Continue in the browser
        {:else}
          Connect
        {/if}
      </button>
    {/if}

    <p class="fine">
      The password or token is kept on {machine}, in a file only your user can read.
    </p>
  </form>
</div>

<style>
  .setup {
    flex: 1;
    display: flex;
    justify-content: center;
    overflow-y: auto;
    padding: var(--space-8) var(--pad-x);
    background: var(--panel-bg);
  }

  .card {
    width: 100%;
    max-width: 30rem;
    height: max-content;
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
    padding: var(--space-8) var(--space-8) var(--space-7);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-xl);
    box-shadow: var(--shadow-sm);
  }

  .mark {
    width: 3rem;
    height: 3rem;
    border-radius: var(--radius-lg);
    display: grid;
    place-items: center;
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }

  h1 {
    margin: 0;
    font-size: var(--text-3xl);
    font-weight: var(--weight-semibold);
  }

  .lede {
    margin: calc(var(--space-4) * -1) 0 0;
    color: var(--fg-muted);
    font-size: var(--text-md);
    line-height: var(--leading-normal);
  }

  .tabs {
    display: flex;
    padding: 0.2rem;
    gap: 0.2rem;
    border-radius: var(--radius-lg);
    background: var(--panel-bg);
    border: 1px solid var(--border);
  }

  .tabs button {
    flex: 1;
    padding: 0.4rem;
    border: none;
    border-radius: var(--radius-md);
    background: transparent;
    color: var(--fg-muted);
    font: inherit;
    font-size: var(--text-md);
    cursor: pointer;
  }

  .tabs button.on {
    background: var(--surface);
    color: var(--fg);
    font-weight: var(--weight-medium);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.08);
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .row {
    display: flex;
    gap: var(--space-4);
  }

  .grow {
    flex: 1;
  }

  .port {
    width: 6rem;
  }

  .providers {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }

  .chip {
    padding: 0.25rem 0.7rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-pill);
    background: var(--surface);
    color: var(--fg);
    font: inherit;
    font-size: var(--text-sm);
    cursor: pointer;
  }

  .chip:hover {
    border-color: color-mix(in srgb, var(--accent) 50%, var(--border));
  }

  .chip.on {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    color: var(--accent);
  }

  .check {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    font-size: var(--text-sm);
    color: var(--fg-muted);
  }

  .note {
    margin: 0;
    font-size: var(--text-md);
    line-height: var(--leading-normal);
  }

  .linkish {
    border: none;
    background: none;
    padding: 0;
    color: var(--accent);
    font: inherit;
    cursor: pointer;
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .steps {
    margin: 0;
    padding: var(--space-4) var(--space-4) var(--space-4) 1.8rem;
    background: var(--panel-bg);
    border-radius: var(--radius-md);
    font-size: var(--text-sm);
    line-height: var(--leading-normal);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  code {
    font-family: var(--font-mono);
    font-size: 0.92em;
    background: var(--code-bg);
    border-radius: var(--radius-xs);
    padding: 0 0.25rem;
  }

  .waiting {
    display: flex;
    gap: var(--space-5);
    align-items: flex-start;
    padding: var(--space-5);
    border-radius: var(--radius-md);
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }

  .waiting p {
    margin: var(--space-2) 0 0;
    color: var(--fg-muted);
    font-size: var(--text-sm);
  }

  .waiting a {
    color: var(--accent);
  }

  .spinner {
    flex: none;
    width: 1.1rem;
    height: 1.1rem;
    margin-top: 0.1rem;
    border: 2px solid var(--border);
    border-top-color: var(--accent);
    border-radius: var(--radius-circle);
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .submit {
    margin-top: var(--space-2);
  }

  .fine {
    margin: 0;
    text-align: center;
    font-size: var(--text-2xs);
    color: var(--fg-muted);
  }
</style>
