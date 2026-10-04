# Notes, Calendar and Mail are Spaces in one app, kept in open formats

The app is becoming the place the user does all their work, not only where they run Agents. So it gains three more **Spaces** next to Agents: Notes, Calendar and Mail. A thin rail of icons down the very left of the window switches between them. Agents is the first icon, and the window opens on it. Each Space has the whole window to the right of the rail, so it can have its own list and pane the way Agents has its sidebar and transcript. The rail remembers the last Space the user was in. The first frame's layout depends on that, so it is read in the pre-paint script in `app.html` (ADR 0015). Ctrl/Cmd+1 to 4 switch Spaces. On a phone the rail becomes a tab bar along the bottom, because ADR 0016 already gives a phone one thing at a time.

**One app and one Host, not a service for each Space.** It was tempting to make each Space its own piece of software that any app could use. But there is no second app, and every separate service would bring its own process, protocol version, authentication and install. ADR 0009 shows what a single window-and-Host version mismatch costs, and this would mean four of them. The Spaces are also most useful when they touch, for example "send this email to an Agent" or "the Agent reads my project notes". That is easiest when they share one process and one store. So each Space is a feature module on the Host, beside `runtime/`: `notes/`, `calendar/`, `mail/`. Its calls go in `host/calls.rs` and `api.ts` like every other, and its UI in `src/lib/<space>/`. The Host and the window keep the boundary already drawn between them, and gain no new boundary.

**The data is open, so other software can still read it.** We get "any app can use it" from formats and protocols that already exist, not from an API of our own:

- **Notes** are Markdown files in a folder, one file per note, and the folder is the whole store. No database and no index file that only we can read. The user's editor, `grep`, git, Syncthing or an Agent with no special tools can all read and write them. If a file changes under the app, the app shows the new version. The default folder is `~/Notes`, and the user can pick another.
- **Calendar** keeps the user's provider as the source of truth. We talk to it over CalDAV and iCalendar, or the provider's own API where it requires one (Google). Whatever the Host keeps is a cache it can throw away. The phone's own calendar app and this one always agree, because neither owns the data.
- **Mail** works the same way, over IMAP and SMTP, or JMAP where the provider has it. The server is the source of truth, and the Host keeps only a cache.

Notes and the Calendar and Mail accounts live on one Host: by default the user's own machine, which can be changed in settings. That Host is always on, a phone has no Host (ADR 0016), and one place means one copy. Account credentials are kept in a file in that Host's state directory, readable only by the user. That file is trusted for the same reason the socket is: its permissions are the trust (ADR 0009).

**Agents reach every Space through MCP.** The app's binary run with `--mcp` is an MCP server over stdio. It forwards each call to the Host's socket. Each Turn gets it through `--mcp-config`, the same way each Turn gets Options through `--settings`, so Agents can search notes, read the calendar and search mail without any setup. Any other MCP client, such as Claude Desktop or a plain `claude` in a terminal, can add the same command. Reading is free. Writing a note is allowed, since it is a local file the user can see and undo. **Anything that leaves the machine is the user's to send:** an Agent can draft an email, a calendar event or an RSVP, but only the user sends it, from the Space, just as only the user Merges or Pushes (ADR 0014).

**Considered and rejected:**

- A separate service per Space. There is no second consumer, and it would cost four protocols to keep in step.
- A database of our own for notes. The files would stop being readable by anything else, and that readability is the whole point.
- Building our own mail or calendar server. The protocols already are the building blocks.

**Consequences.**

- The app is no longer only a control layer for Claude Code. CLAUDE.md's goal and CONTEXT.md should say so.
- "Event" already means a stream event in this codebase, so a calendar entry is always a **Calendar event**, both in code (`CalendarEvent`) and in the glossary, and never just an "event".
- Google treats full mail access as a "restricted" scope. That is fine for the user's own account in an unverified OAuth app, but giving the app to other people would need Google's paid security review.
- Repeating events, time zones, mail threading and safely rendering untrusted HTML mail are each hard. Calendar and Mail start narrow: reading, searching and "hand this to an Agent" come before writing and sending.
- Order of work: the rail, then Notes (cheapest, and the most use to Agents), then MCP, then Calendar, then Mail.
