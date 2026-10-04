# Connecting Google Calendar

The Calendar Space reads Google Calendar through Google's own API (ADR 0017). Google only lets an app do that while signed in as an **OAuth client**, and the app doesn't ship one: you make your own, once, in your own Google Cloud project. That keeps your calendar between you and Google, and it costs nothing.

It takes about five minutes. You need a browser on the machine whose Host keeps your calendars (usually this one): Google sends the browser back to that machine when you sign in.

## 1. Make a Google Cloud project

1. Open <https://console.cloud.google.com/> and sign in with the Google account whose calendar you want.
2. In the project picker at the top, choose **New project**. Call it anything, for example `Orchestrate`, and create it. Make sure it's the selected project afterwards.

## 2. Turn on the Calendar API

1. Go to **APIs & Services → Library** (<https://console.cloud.google.com/apis/library>).
2. Search for **Google Calendar API**, open it and press **Enable**.

## 3. Set up the sign-in screen

1. Go to **Google Auth Platform → Branding** (<https://console.cloud.google.com/auth/branding>). If it asks you to get started, do:
   - App name: `Orchestrate`. User support email: your address.
   - Audience: **External**. If your account is a Google Workspace one, you can pick **Internal** instead and skip step 3 below.
   - Contact email: your address. Agree and create.
2. Go to **Data Access** and press **Add or remove scopes**. Find `.../auth/calendar.readonly` ("See and download any calendar you can access"), tick it, update and save. This is the only thing the app asks for: it never changes your calendars.
3. Go to **Audience**. Under **Publishing status**, press **Publish app** and confirm.

   Why: while an External app is still "Testing", Google expires its sign-ins after seven days, and you'd be asked to sign in again every week. A published app keeps working. Google won't review it, since it's yours alone, so the sign-in will say the app isn't verified. That's expected (see step 5).

## 4. Make the OAuth client

1. Go to **Google Auth Platform → Clients** (<https://console.cloud.google.com/auth/clients>) and press **Create client**.
2. Application type: **Desktop app**. Name: `Orchestrate`. Create.
3. Copy the **Client ID** and the **Client secret** it shows you. You can also press **Download JSON** to keep a copy.

## 5. Connect it in Orchestrate

1. Open the Calendar Space and press **Connect Google Calendar** (or **Accounts** at the bottom of the side panel, then **Google**).
2. Paste the client ID and the client secret, and press **Save client**.
3. Press **Sign in with Google**. Your browser opens on Google's sign-in.
4. Pick your account. Google says **"Google hasn't verified this app"**: press **Advanced**, then **Go to Orchestrate (unsafe)**. It's your own app, so this is safe.
5. Allow it to see your calendars. The browser says **Google Calendar is connected**, and your calendars appear in the app within a few seconds.

Instead of pasting in step 2 you can put the downloaded JSON file on the Host as `calendar/google-client.json` in its state directory (`~/.local/state/orchestrate/calendar/google-client.json` on Linux), readable only by you (`chmod 600`).

## What's kept, and where

Everything stays on the Host, in `calendar/` under its state directory:

- `google-client.json`: the client ID and secret.
- `accounts.json`: the sign-in Google gave back (a refresh token).
- `cache/`: a copy of your Calendar events, which the Host can throw away and fetch again.

All three are readable only by your own user, the same way the Host's socket is. Google stays the source of truth. The Host asks it for changes every five minutes, and whenever you press the sync button.

## Disconnecting

**Accounts → Remove** forgets the account on the Host. To also revoke the app's access on Google's side, remove **Orchestrate** at <https://myaccount.google.com/permissions>.

## When something goes wrong

- **"Google wants you to sign in again"**: the sign-in expired or was revoked. If it happens every week, the app is still in Testing (step 3.3). Press **Sign in again** under Accounts.
- **"connect Google from a window on the Host's own machine"**: Google sends your browser back to `127.0.0.1`, which only reaches the Host from its own machine. Sign in there once. Afterwards every window, phones included, sees the calendar.
- **"Access blocked: … has not completed the Google verification process"**: the app is in Testing and your address isn't a test user. Publish it (step 3.3), or add your address under **Audience → Test users**.
- **"Google Calendar API has not been used in project …"**: step 2 was skipped, or done in another project.
