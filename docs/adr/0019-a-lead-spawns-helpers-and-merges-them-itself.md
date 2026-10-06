# A Lead spawns Helpers, and merges their work into its own branch

The user wants one Agent to split its Task and hand the parts to other Agents that work at the same time. Claude Code's own subagents (its Agent tool) already let an Agent fan out, and for reading and searching they are enough. For changing code they fall short. They all edit the one Worktree, so they collide. The app can't see them, so the user can't watch, Stop or Resume one. And their work doesn't come back as a diff of its own.

So an Agent may Spawn other Agents of the app, over the MCP server (ADR 0017). The one that Spawns is the **Lead** and the ones it Spawns are its **Helpers**. A Helper is an ordinary Agent with its own Worktree, branch, Session and transcript, and the user can do anything with it that they can with any other. The only difference is that it records its Lead, as `lead_id`.

**The tools.** The Agents Space gets four more tools:

- `spawn_helper` Spawns a Helper on a Task the Lead writes.
- `wait_for_helpers` returns once the Lead's Helpers have all finished, with each one's State, branch and last answer.
- `send_to_helper` gives a Helper a follow-up, which waits in its Queue if it is still working.
- `stop_helper` Stops one.

`spawn_helper`, `send_to_helper` and `stop_helper` write, so they run only in a Turn that may write anyway, never in plan mode. All four act only on the calling Lead's own Helpers. The MCP server knows which Agent is calling because each Turn's `--mcp-config` names it in the server's environment.

**How a Helper starts.** On the Lead's Host and in its Project, with its branch cut from the Lead's current commit. That commit is also its Base, so its diff is only its own work. It takes the Lead's Mode, Options, Model and Effort, unless the Lead picks another Model or Effort. So a Helper never runs looser than its Lead. Every Turn of a Helper gets a short system prompt (`--append-system-prompt`) telling it that it works for a Lead, to Commit its work on its own branch when done, and that its final answer is its report. Its Task stays exactly what the Lead wrote, as any other Agent's does.

**How the work comes back.** The Lead merges each Helper's branch into its own, by running `git merge` in its own Worktree. Then the user reviews and Merges the Lead alone, as they would any other Agent. This doesn't break the rule that only the user Merges (CONTEXT.md, Merge). That rule is about the Project's own branches, and the Lead's branch is a scratch branch like every Agent's. The Merge picker still offers no `cw/agent-*` branch, so the user never merges one Agent into another by hand. Because the Lead does the integrating, it also settles any conflicts between its Helpers before the user sees the result.

**Limits.**

- **One level.** A Helper can't Spawn Helpers. The Host refuses, and a Helper's Turns aren't offered the tools.
- **At most eight Helpers working at once per Lead.** Each one is a `claude` spending against the same account, and a model asked to parallelise will happily start thirty.
- **A Mail-locked Agent can't lead (ADR 0018).** The Task it wrote would carry what it read in the mail to an Agent that isn't locked. Its Turns aren't offered the writing tools either.

**What the user sees.** Each Helper sits under its Lead in the sidebar, and the team shares the Lead's Bucket. It sits under Running while any of them is working. Stopping a Lead Stops its working Helpers too, because the user stopping the Lead means the whole job should stop. A Helper can still be Stopped or Resumed on its own. A Helper that Completes doesn't notify the user, because its Lead hears of it. A Helper that Fails still does. Discarding a Lead leaves its Helpers, which then stand on their own.

**Considered and rejected:**

- **Letting the user Merge Helpers one by one, into their own branches.** This needs no new rule, but five Helpers means five Merges, and nobody settles conflicts between them before the user does.
- **A Merge tool for the Lead.** The Lead already has git, in a Worktree that is its own, so a tool would only rebuild `git merge`.
- **Spawning Helpers from the Project's tip, as a user's Spawn does.** Then they wouldn't see what the Lead had already done, and their diffs would be against a commit the Lead's branch has moved past.
- **Helpers of Helpers.** A tree of Agents is hard to follow in a sidebar and hard to stop. One Lead with a flat team covers the parallel work the user asked for.
