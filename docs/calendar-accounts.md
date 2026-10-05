# Connecting calendars

The Calendar Space reads and writes your calendars where they already live (ADR 0017): Google Calendar, Outlook.com or Microsoft 365, or any CalDAV server. The provider stays the source of truth. The Host only keeps a copy, which it can throw away and fetch again.

Open the Calendar Space and press **Accounts** at the bottom of its side panel, or one of the buttons it shows when nothing is connected yet.

## Google and Outlook: sign in

Press **Sign in with Google** or **Sign in with Microsoft**. Your browser opens on the provider's sign-in page. Pick your account and allow the app to see and change your calendars. The browser then says the calendar is connected, and it appears in the app a few seconds later.

- Do this from a window on the machine whose Host keeps your calendars, usually the one you're on. The provider sends your browser back to that machine. After that, every window sees the calendar, phones included.
- Outlook covers Outlook.com, Hotmail and Live accounts, and work or school Microsoft 365 accounts.
- The app can make, change and delete Calendar events, and invite people. It only does so when you do it from the Calendar Space. Agents can read your calendars and *draft* a Calendar event, which waits in the Space, dashed, until you send it or throw it away.

If the button asks for an app instead, this build came without one. See [Registering the apps](#registering-the-apps-once) below, or paste your own.

## CalDAV: a server, a user name and a password

Fastmail, iCloud, Nextcloud, Zoho, Yahoo, Radicale and most others. Use an app password where the provider offers one:

| Provider | Server | Password |
|---|---|---|
| iCloud | `https://caldav.icloud.com` | An app-specific password from account.apple.com |
| Fastmail | `https://caldav.fastmail.com` | An app password with CalDAV access |
| Nextcloud | `https://your.cloud/remote.php/dav` | An app password from Settings → Security |
| Yahoo | `https://caldav.calendar.yahoo.com` | An app password |

Whether invitations are emailed is up to the server. iCloud, Fastmail and Nextcloud send them themselves. A server that doesn't (Radicale, for one) only puts the people on the Calendar event, and the editor says so before you save.

## What's kept, and where

Everything stays on the Host, in `calendar/` under its state directory (`~/.local/state/orchestrate/calendar/` on Linux), readable only by your own user:

- `accounts.json`: each account's sign-in (a refresh token, or a CalDAV password).
- `google-client.json`, `microsoft-client.json`: apps you registered yourself, if any.
- `cache/`: a copy of your Calendar events.
- `drafts.json`: Calendar events Agents drafted for you.

The Host asks each provider for what changed every five minutes, after every change you make, and whenever you press the sync button.

**Accounts → Remove** forgets an account on the Host. Nothing at the provider is touched. To revoke the app's access there as well, use <https://myaccount.google.com/permissions> or <https://account.live.com/consent/Manage> (or, for a work account, <https://myapps.microsoft.com>).

## Registering the apps, once

Signing in with Google or Microsoft needs an app registered with each. As the app's author, you do this once. Then build with the IDs, and everyone using that build only presses **Sign in**:

```sh
export ORCHESTRATE_GOOGLE_CLIENT_ID=1234-abc.apps.googleusercontent.com
export ORCHESTRATE_GOOGLE_CLIENT_SECRET=GOCSPX-...
export ORCHESTRATE_MICROSOFT_CLIENT_ID=00000000-0000-0000-0000-000000000000
npm run tauri build   # or `npm run tauri dev`; a rebuild picks up changes
```

For CI, add the three as repository secrets and pass them in the build step's `env:`. A Google "secret" for an installed app isn't a secret: Google says so, and it ships inside the app either way. The Microsoft app has no secret at all.

Anyone can instead register their own and paste it into the Accounts dialog (**Use your own app**). The Host then keeps it in `google-client.json` or `microsoft-client.json`, which takes precedence over the build's.

### Google

1. Open <https://console.cloud.google.com/>, and in the project picker make a **New project** (for example `Orchestrate`).
2. **APIs & Services → Library**: find **Google Calendar API** and **Enable** it.
3. **Google Auth Platform → Branding**: app name `Orchestrate`, your support and contact email. Audience **External**, or **Internal** for a Google Workspace organization of your own.
4. **Data Access → Add or remove scopes**: tick `.../auth/calendar.readonly` and `.../auth/calendar.events`, then save.
5. **Audience → Publish app**. While an External app is still "Testing", Google expires its sign-ins after seven days.
6. **Clients → Create client**: type **Desktop app**, name `Orchestrate`. Copy its client ID and secret.

An app Google hasn't verified shows **"Google hasn't verified this app"** at sign-in: press **Advanced → Go to Orchestrate**. It also allows at most 100 users. Giving the app to more people than that, or without the warning, takes Google's verification of the two calendar scopes. That review is free for these scopes (ADR 0017: unlike Mail's restricted ones).

### Microsoft

1. Open <https://entra.microsoft.com/> → **App registrations → New registration**. With a personal Microsoft account you may need to make a free Azure account first: <https://azure.microsoft.com/free>.
2. Name `Orchestrate`. Supported account types: **Accounts in any organizational directory and personal Microsoft accounts**.
3. Redirect URI: platform **Public client/native (mobile & desktop)**, `http://localhost`. Any port on it works, which is how the Host listens.
4. Register, and copy the **Application (client) ID**.
5. **API permissions → Add a permission → Microsoft Graph → Delegated**: `Calendars.ReadWrite`, `User.Read` and `offline_access`. They don't need an admin's consent.
6. **Authentication**: leave **Allow public client flows** off; signing in uses the browser, not a device code.

A work or school organization may only let its users sign in to apps from a verified publisher, or that an admin has approved. If sign-in says *"Need admin approval"*, ask your IT admin to approve **Orchestrate**, or verify the publisher under **Branding & properties** (it takes a Microsoft Partner Network ID).

## When something goes wrong

- **"… wants you to sign in again"** under Accounts: the sign-in expired or was revoked. Press **Sign in again**. A Google account connected before the app could write asks for this once, on its first change, to allow changing Calendar events.
- **"sign in from a window on the Host's own machine"**: the provider sends your browser back to `localhost`, which only reaches the Host from its own machine. Sign in there once.
- **"Access blocked: … has not completed the Google verification process"**: the Google app is still in Testing and your address isn't a test user. Publish it (step 5), or add yourself under **Audience → Test users**.
- **"it changed on the server since it was last synced"**: someone changed the Calendar event elsewhere while you were editing. The app refuses to overwrite it. Sync, then make your change again.
