# Claude Code Control Layer

A desktop app for all of the user's work. At its centre it spawns, monitors and controls Claude Code agents running in isolated Git worktrees, and it manages the agents around Claude Code, not the coding work itself. Next to the Agents are the user's notes, calendar and mail (ADR 0017).

## Language

**Space**:
One of the app's top-level areas, which the user switches between with the rail of icons on the far left: Agents, Notes, Calendar and Mail. Each Space has the whole window to the right of the rail. Its data lives on a Host in an open format other software can read: Markdown files for Notes, and the user's own calendar and mail servers for Calendar and Mail (ADR 0017). Agents reach every Space through the app's MCP server.
_Avoid_: App (the whole thing is the app), Module (that's the code), Tab (the Diff tab is one), Section, View, Page.

**Agent**:
One `claude` process, bound to one Worktree, running one Task. Its lifetime is the process's lifetime; when the process ends, the Agent ends.
_Avoid_: Session (has a different, later meaning), Runner, Job, Worker.

**Task**:
The work item a user hands an Agent at Spawn — its opening prompt. An Agent has exactly one Task for its whole life; follow-ups refine it rather than replace it. The Task is what the Agent was asked to do, not what the UI calls it — that's the Title.
_Avoid_: Job, Prompt (too narrow — the Task is the *unit of work*, not just the string), Instruction.

**Title**:
The short name the UI identifies an Agent by, written by Claude from the Agent's own work: at the end of a Turn we hand a cheap, isolated `claude` the Task and the Turn's answer and ask for a few words. Written at the end of each of the first three Turns and then frozen — a name that kept shifting would cost the user the thing a name is for, which is finding the same Agent again tomorrow. Until the first Turn ends, and on Agents recorded before Titles existed, the first line of the Task stands in. Ours rather than Claude Code's own `ai-title`: that one is only written for interactive sessions, and it is shared across the sessions in a directory, so sibling Agents would all wear the same name.
The user can give their own with `/rename`, which wins and is never rewritten. It is kept apart from Claude's, so `/rename` with no name hands the Agent back to Claude's Title.
_Avoid_: Name (too close to the Project's), Summary, Label, Description.

**Tag**:
A colour the user marks an Agent with, from `/color`: a dot before its Title in the sidebar, and a tint on its composer, the way Claude Code's `/color` tints its prompt bar. The colours are Claude Code's own `/color` choices, so the command takes what a Claude Code user already types. A way to find an Agent again at a glance, and nothing more: no Bucket, filter or State reads it.
_Avoid_: Label, Colour on its own, Category.

**Bucket**:
The heading an Agent sits under in the sidebar tree. Usually its State, but an Agent whose work has been Merged is bucketed as **Delivered** instead: where the work went says more to someone scanning the list than how the last Turn happened to end, and an Agent already in the Project is one you are done with rather than one waiting on you. Delivered sits last, under the states. Running outranks the merge record — an Agent that is working is the thing you most need to see, wherever its previous Turn's work ended up — so an Agent Resumed out of Delivered climbs to Running and drops back when the Turn ends. A Bucket is a way of reading an Agent, not a property of one: it leaves Merge a record rather than smuggling in a state. Delivered means Merged *and* holding nothing since: an Agent that was Resumed and left work uncommitted, or Committed again, drops back under its own State — normally Completed — until that work is Merged too, because the pile you are done with is no place for an Agent still carrying something. Its own State rather than always Completed: one that Merged, was Resumed, and then Failed belongs under Failed. Reading this costs the sidebar a git call the Agent record can't answer, so it is asked only of Agents that already have a merge record, and only when a Turn ends or a Commit lands — the two moments a Merged Agent's Worktree can have changed.
_Avoid_: Group, Category, Section, Status (State already means that), Merged (the record's word — the bucket is Delivered).

**Turn**:
One prompt-and-answer exchange within an Agent: one `claude` process, from Spawn or Resume until it exits. Turn 1 carries the Task; later Turns carry follow-up prompts. Turns are counted on the Agent and recorded in its log, so the output pane reads as a conversation.
_Avoid_: Run, Iteration, Round, Message (a Turn contains many messages).

**Row**:
One entry in the output pane. A Row is not an event: a burst of stream events collapses into one, because a transcript that prints a card per event is unreadable within a minute of the Agent starting. Three rules do the collapsing — a tool call and its result are one Row (they arrive as two events but are one act); a run of consecutive calls to the same tool is one Row, labelled `Read ×8 — api.ts, store.svelte.ts, +6` and opening to the individual calls; and a stretch of consecutive thinking is one Row. A Row keeps the key of the first event it covers, so a Row that grows while the Turn runs keeps whatever the user had expanded open. Grouping is live — a Row's count climbs as calls land, which is the point: repeated work overwrites itself instead of pushing the answer off screen.
_Avoid_: Card, Line, Entry, Event (the Row is what's rendered; the event is what arrived).

**Snapshot tool**:
A tool whose input is a whole state rather than an action — `TodoWrite` is the one we know of. The tenth todo list supersedes the nine before it, so only the last one in a Turn gets a Row and the earlier ones vanish rather than stacking. Scoped to the Turn, not the transcript: reading back an old Turn should show the list as it stood when that Turn ended, not the one from today.
_Avoid_: Idempotent tool, State tool.

**Queue**:
What the user has said to an Agent that wasn't free to hear it yet: prompts typed while a Turn was still running, held in order and sent as their own Turns once the Agent is free. A Queue belongs to one Agent, is always visible above the composer with its messages readable and individually removable, and drains one message per clean Complete — a Stop or a Fail holds the rest, since interrupting an Agent shouldn't fire the rest of the line into it. Kept on the Agent's Host rather than in any window, so it drains while no window is watching and every window shows the same one. The Host, not the window, decides whether a message starts a Turn or joins the Queue, so two windows sending at once can never start two Turns in one Worktree. It is still ours, not part of the conversation Claude Code owns: a record of what the user means to say.
_Avoid_: Buffer, Pipeline, Inbox.

**Attachment**:
A file handed to an Agent along with a prompt — at Spawn, at Resume, or on a queued message. You can drop it anywhere on the window, pick it with the paperclip, or paste it as an image. An Attachment is a copy, kept on the Agent's Host: the window sends the file's bytes along with the prompt, whether the Host is across the network or the same machine, so a dropped file behaves one way wherever the Agent runs. `claude` reads it with its own tools, which already handle code, images and PDFs. So the Turn's prompt lists the copies' paths under the user's text, and their folder goes to `--add-dir` so that a plan-mode Turn may read outside its Worktree too. The transcript records Attachments next to the prompt rather than inside it, so they show as files rather than as a list the user never typed.
_Avoid_: Upload (as a noun — the Attachment is the file, not the transfer), Context file, Reference.

**Slash command**:
A prompt that starts with `/name`: a Claude Code built-in like `/compact`, or one of the user's or the Project's skills and commands. It goes out as the Turn's prompt like any other, and `claude` is what runs it. The composer's `/` menu offers the ones `claude` lists for the directory the Turn would run in — the Agent's Worktree, or the Project for a new Agent — asked afresh each time the menu opens, since a Project's own commands live in its tree and skills get added mid-session.
Every Turn is a new `claude` that exits when it's done. So a command that changes the terminal, or a setting for the session, would be lost as soon as that Turn ended. The app answers those commands itself, and they are marked "app" in the menu. `/model` and `/effort` move the pickers. `/advisor` and `/output-style` set the Agent's Options. `/rename` sets the Title and `/color` the Tag. `/theme` and `/usage` act on the app. `/plugin`, a whole screen in a terminal that `claude --print` doesn't have, opens the app's Plugins window. It has Discover, Installed and Marketplaces tabs, and every change goes through `claude plugin` on the Agent's Host. Typed forms like `/plugin install x@market` work too. A plugin applies to each Agent from its next Turn, since every Turn loads plugins anew. A command a marketplace wants run to install a plugin is shown to the user, and runs only once they agree to it. `/commit` and `/merge` are the Diff tab's Commit and Merge, typed: `/commit` with no message uses the Agent's Title as the subject, and `/merge` with no branch goes where the Merge picker points. The app's `/commit` hides any `/commit` skill the user or the Project has. Commit is the user's action here, and anyone who wants Claude to write the commit can ask it to in words. `/clear` goes to `claude`, which moves on to a new Session. The Agent follows it there, and the transcript is cleared with it, so the next Turn starts empty. Commands with no use here are left out of the menu, and turned back with the reason if they are typed. That covers `/loop`, which dies with its Turn; `/batch`, which would spawn agents the app can't see; `/fast`; and a few reload and debugging commands.
_Avoid_: Command on its own (too close to a Tauri command), Macro, Action.

**Options**:
The Claude Code settings an Agent runs every Turn with: its advisor (the model Claude consults at key moments) and its output style. They are set for the Agent, not picked per prompt. You set them with `/advisor` and `/output-style`. They are handed to each Turn as `claude --settings`, layered over the user's own settings. Each one is optional, and an unset one is left to Claude Code's own configuration. A new Agent spawns with the Options last set on any Agent, because they are meant to stick. They can be changed while a Turn runs, like the Title and the Tag, and take effect from the next Turn.
_Avoid_: Settings (that's Claude Code's own file), Preferences, Config.

**Host**:
A machine Agents run on: it owns their `claude` processes, Worktrees, Sessions and logs, and keeps them working whether or not any window is open. Every Agent belongs to exactly one Host for its whole life, because its Session and Worktree cannot leave that machine; its work can, by Hand off. A window can show and control Agents on any Host it can reach; the machine the window runs on is usually a Host too. A phone never is, since it can't run `claude`, so the app there shows other machines' Hosts only (ADR 0016). The Host's own disk is the only record of its Agents. When a Host is off or unreachable, or runs a different version from the window, its Agents stay listed as they were last seen, dimmed and with every control disabled, because hiding them would read as if they had been Discarded.
_Avoid_: Runner (an old near-synonym for Agent), Server, Machine, Device (that's what you're holding), Node, Remote.

**Project**:
A Git repository the user has explicitly registered with the app. A Project with a remote is identified by it, so its checkouts on several Hosts are one Project: its Agents sit together under one heading whichever Host they run on, and the Host is picked at Spawn. On each Host its Agents branch from the checkout the user registered there, or, where there is none, from one the Host clones from the remote itself, so any Host can take any such Project without setting it up first. Registering a checkout later takes over from the clone for new Agents. A Project with no remote exists on one Host only. Registering is also the user's one decision to trust it: `claude --print` skips Claude Code's own workspace trust prompt, so the repository's `CLAUDE.md`, hooks and settings take effect in every Agent unasked. The app says so and asks once, when the Project is added — once per Project, not once per Host, since what is being trusted is the repository, and it is the same on every machine. Projects already registered are not asked again. A folder that isn't a Git repository with a commit yet can still be added: the same confirmation sets it up (`git init`, a default `.gitignore` if it has none, and a first commit of its files), since Agents can only branch from a commit. A Project may contain many Agents over time (spawned across separate Worktrees), and no Agent ever touches the Project's own working tree. Removing a Project unregisters it on every Host that can be reached, and names the ones that couldn't; its Agents and checkouts stay until they are Discarded. The app touches the Project only when the user asks it to: a Merge advances one of the Project's branches, and reaches its working tree only when the branch being merged into is the one checked out.
_Avoid_: Workspace, Repo (in UI), Directory.

**Worktree**:
An isolated Git worktree created for one Agent, living under a global scratch directory outside the Project. The Worktree is the Agent's sandbox; it is destroyed when the Agent is Discarded.
_Avoid_: Checkout, Clone, Branch dir.

**Base**:
The commit an Agent's branch was cut from at Spawn, recorded on the Agent. On a Project with a remote, Spawn fetches first and cuts from whichever of the local branch and its upstream is newer when one contains the other. When they have diverged it cuts from the local branch, and the transcript says how many commits that leaves out. Either way the Project's checkout is never moved. Everything the Agent produced is expressed as a diff against its Base, so committed and uncommitted work read as one change set even if the Project's own branch moves on afterwards.
_Avoid_: Parent, Fork point, Origin.

**Model**:
Which Claude model an Agent's Turns run on, recorded on the Agent and passed to `claude --model` as a full model name (`claude-opus-4-5-20251101`), version included, so an Agent keeps running on the model the user actually picked. The choices come from Anthropic's Models API, asked with the same credential `claude` itself uses — the list is the user's own account, not one baked into this app, so it covers models released after any given build and never offers one the Agent couldn't run. Picked at Spawn and changeable at Resume: starting cheap and escalating is a normal move, so the Model belongs to the Turn as much as to the Agent. Unset means we pass no `--model` at all and Claude Code's own configured default applies; that is also the fallback when the account can't be reached, so the picker always has a working choice. The last Model the user picked for any Turn is remembered in the window's local storage and is what the next Spawn opens on — people work on one model for stretches at a time, so re-picking it every Spawn is friction; on a profile that has no record yet, the most recently spawned Agent's Model stands in.
_Avoid_: Engine, Backend, Tier.

**Effort**:
How hard an Agent's Turns think, passed to `claude --effort` as one of `low`, `medium`, `high`, `xhigh`, or `max`. It rides with the Model rather than claiming a control of its own — it is picked from a submenu hanging off each model row, because how capable a model is and how hard it works are one decision about what a Turn is worth. Like the Model it is picked at Spawn, changeable at Resume, carried on each queued message as it stood when that message was queued, and remembered in the window's local storage, falling back to the most recently spawned Agent's Effort on a profile with no record. Unset means we pass no `--effort` at all and Claude Code's own level applies; that is also what Agents recorded before Effort existed get.
_Avoid_: Thinking budget, Reasoning level, Depth, Quality.

**Permission Mode**:
What an Agent's Turns may do without asking, passed to `claude --permission-mode`. Two choices: `bypassPermissions` — **YOLO** in the UI — the default, where the Agent acts freely inside its Worktree, and `plan`, where it reads and proposes but writes nothing. The picker uses YOLO rather than the flag's own word because it says what the choice feels like to make: the Agent is off the leash, and the Worktree is the only thing between it and the Project. The modes Claude Code offers that stop to ask are deliberately absent — we hand `claude` no input channel, only `--output-format stream-json`, so a permission prompt would hang the Turn with nowhere to answer it. Isolation rather than permission is the safety story here: the Worktree is the sandbox, which is why acting freely inside one is the default, and `plan` exists for the Agent you want to think before it touches anything. Picked at Spawn, changeable at Resume, and carried on each queued message as it stood when that message was queued, exactly as the Model and Effort are. A plan-mode Agent still gets a Worktree and still reads the Project's code; it simply produces nothing to Commit or Merge, and the UI says so rather than showing an empty diff.
_Avoid_: Mode (too vague on its own), Bypass (the flag's word, not the UI's — say YOLO), Sandbox (that's the Worktree), Approval, Safety level.

**Session**:
The Claude Code conversation history behind an Agent, named by the UUID we mint at Spawn and pass as `--session-id`. Claude Code owns the transcript; we only keep the ID, and Resume hands it back via `--resume`. A Session is scoped to the directory it started in, which is why it survives exactly as long as the Agent's Worktree does.
_Avoid_: using it as a synonym for Agent (an Agent is the thing the user talks to; the Session is the history that makes talking again possible), Thread, History, Context.

**Usage**:
What Turns have cost: tokens and dollars per Model, plus how much of the account's rate-limit windows is spent. Read back out of the Agent logs rather than tallied as events arrive — Claude Code reports a Turn's per-model totals on its `result` event and the account's windows on `rate_limit_event`, so the logs are the record and the numbers are right after a restart. Account-wide rather than per-Project or per-Host: every Agent spends against the same limits, which is why the usage window totals across Agents on every reachable Host by default, and why a log that outlived its Discarded Agent still counts toward the total.
_Avoid_: Cost (only half of it), Quota, Budget (nothing here enforces one), Stats.

**Conversation**:
In the Mail Space, the messages in a folder that answer each other, listed as one row and read together. They are joined by Gmail's own thread id where the server is Gmail, and otherwise by the References and In-Reply-To headers, never by subject, since two different mails called "Invoice" are not one conversation. Archiving, deleting, marking read and **Send to agent** act on the whole Conversation. Send to agent Spawns an Agent in a Project the user picks, with the Conversation quoted as plain text as its Task. The Task tells the Agent that the mail is information to work with, not instructions, since anyone can write an email. In code it is a `Thread`.
_Avoid_: Thread (in the UI), Chain, Email (that's one message).

## Agent lifecycle

An Agent moves through these states, and each transition has a specific verb.

**Spawn**:
The transition that creates a new Agent on a Host of the user's choosing (by default the one last used for that Project): allocate a Worktree, mint a Session ID, start the `claude` subprocess, wire up streaming. After Spawn, the Agent is *running* its first Turn.

**Resume**:
Putting a stopped Agent back to work with a follow-up prompt: start a new `claude` in the Agent's existing Worktree with `--resume <session>`, and return the Agent to *running* for another Turn. Available from Completed, Failed, Stopped, and Orphaned — anything with a Session and a Worktree still on disk. Refused while the Agent is already working; Stop it first to change course. Resume is how a user keeps talking to one Agent instead of Spawning another.
_Avoid_: Continue, Restart (a Resume keeps the conversation; a restart would discard it), Retry, Follow-up (as a verb).

**Stop**:
A user-initiated end to the current Turn. The `claude` process is terminated (SIGTERM, then SIGKILL after a grace period) and the Agent goes to *stopped*. The Worktree and Session are preserved, so a Stopped Agent can be inspected, Committed, or Resumed with a corrected prompt — Stop is how a user interrupts an Agent heading the wrong way. Only Discard destroys anything.
_Avoid_: Kill, Cancel, Abort.

**Complete**:
A Turn's natural end — the `claude` process exits zero on its own. The Worktree is preserved so the user can inspect the diff and Merge or copy work out, and the Session is preserved so the user can Resume with a follow-up. The common case is an Agent that sits Completed between Turns for as long as the user wants.
_Avoid_: Finish, End, Done (as verbs).

**Fail**:
A Turn's unnatural end — the `claude` process exits non-zero, cannot be started at all, is killed by the OS, or otherwise crashes. Distinct from Complete (clean exit) and Stop (user-initiated). Like Complete, the Worktree and Session are preserved: the user can read the stderr in `fail_reason`, then Discard or Resume. The last JSONL events and exit code are recorded.
_Avoid_: Crash, Error (as state names).

**Commit**:
A user action on a Completed, Failed, Stopped, or Orphaned Agent: stage everything in its Worktree and commit it onto the Agent's own branch. This is how work survives a later Discard — the commit stays reachable in the Project's object store — and it is what a Merge needs, since only committed work can be merged. Refused while the Agent is *running*, since it would capture a tree the Agent is still writing. The app never commits on its own; whether an Agent commits its own work is up to Claude Code.
_Avoid_: Save, Merge (that's the next action along — a Commit stays on the Agent's own branch and touches nothing of the user's), Checkpoint.

**Merge**:
Bringing a Committed Agent's branch into a branch of the user's choosing — the only action that writes to the Project, and never one an Agent takes. The target is picked from the Project's local branches, its checked-out branch first and by default, with `cw/agent-*` branches excluded: merging one Agent into another's branch is the multi-agent coordination V1 defers. It is always `--no-ff`, so the Agent's work stays identifiable as one unit in history — that is the point of giving each Agent its own branch, and squashing or rebasing would break the promise that everything an Agent produced reads as one diff against its Base. When the target is not the checked-out branch the merge happens in a throwaway worktree, so a Merge never moves the user off their branch; when it is, it happens in place and is refused against a dirty tree. Refused also while the Agent is working, and while its Worktree holds uncommitted work — Commit first. A conflict aborts, leaving the Project byte-identical and naming the files that collided: this app has no merge tool, and a half-merged Project is somewhere the app could not get it back out of. What it offers instead is to Resolve. On a Project with a remote, a Merge fetches first. A target that is only behind its upstream is fast-forwarded before the merge, unless it is checked out with local changes. One that has diverged from its upstream is refused and named, just as a conflict is. The Merge then pushes the target only if the user turned Push on for it; Push is off by default, since a push leaves the machine (ADR 0014). A Merge into a target that tracks a remote but wasn't pushed, whether the user didn't ask or the push was rejected, is recorded as not yet pushed, and the push is offered. A Merge does not end an Agent; it is recorded on the Agent, with the branch it went into, the way the Base is. The control offers only branches that do not already have the work, and disappears entirely once every branch has it — in its place the diff says where the work went. Committing again brings it back, since the new commits are unmerged. In the sidebar a Merged Agent moves into the Delivered Bucket, which is a way of reading the record rather than a state the Merge put it in — and which it leaves again the moment it holds work the Project has not got.
_Avoid_: Land (what this was called before), Ship, Integrate, Promote.

**Resolve**:
The user's answer to a Merge that conflicted: Spawn a **Resolver** to settle the conflict, and let the app finish the Merge once it has. The Resolver is an ordinary Agent, on the conflicted Agent's Host since that is where its branch lives, with two differences — its branch is cut from the conflicted Agent's rather than from the Project's HEAD, and it records which Agent and target branch it is resolving. Its Task tells it to merge the target into its own branch and settle the conflicts there, in its own Worktree, so the Project is never half-merged and the conflicted Agent's branch is never touched. Its Base is the target's tip, so its diff reads as what the finished Merge will bring in. It always runs YOLO, since a Resolver that may not write can resolve nothing. When its Turn Completes with the target in its branch and nothing left uncommitted, the app Merges the Resolver's branch into the target — the Merge the user already asked for — and records it on both Agents, which puts both under Delivered. The conflicted Agent is recorded only if its tip really is in the target, so one that kept working meanwhile stays where its own State puts it. If the Resolver ends short of that — the target not merged in, work left loose, or the target moved and conflicts again — nothing is merged, the transcript says why, and the user can reply to it or Merge it by hand. That happens once: a Resolver Resumed after its Merge went through is just an Agent.
_Avoid_: Fixer, Conflict agent, Rebase.

**Hand off**:
Carrying a stopped Agent's work to another Host, by Spawning a new Agent there that starts from it. A Project's Agents can't move, because their Session and Worktree are stuck on their Host. So when the user picks another Host for the next prompt, the Agent's own Host pushes what it Committed to the remote, under a ref kept only for Handoffs. The chosen Host Spawns from that ref and takes the ref down again. The new Agent's Task opens with a brief (the old Agent's Task, the answer from its last Turn, and where its work started), followed by the user's prompt. Its Base is the old Agent's, so its diff and Merge cover all of the work. Refused while the Agent holds uncommitted work, since the new Agent would silently go without it: Commit first. Only a Project with a remote can Hand off. The old Agent is left as it was. See ADR 0013.
_Avoid_: Move, Migrate, Transfer (the Agent stays where it is; a new one picks up its work), Continue (that's Resume's avoided word).

**Discard**:
Ends an Agent for good: destroys its Worktree, deletes its branch, and removes it from the app's list. Always an explicit user action, and the only action that destroys anything — so it discards Commits the Agent made as well as uncommitted work, and takes the Session with it (a Session cannot outlive the directory it ran in). Refused while the Agent is working. After Discard, the Agent's on-disk log file (JSONL) is still preserved. The Delivered Bucket can be Cleared in one go from its sidebar heading, which Discards every Agent in it; git is asked afresh first, so an Agent that has picked up work since its Merge is left alone. Clearing is named apart because Delivered work is already in the Project, so nothing the user needs is lost.
_Avoid_: Reap (what this was called before), Cleanup, Delete, Remove, Clear (for a single Agent — it reads as clearing the transcript, like Claude Code's `/clear`).

**Orphan**:
An Agent that was working when its Host stopped running Agents — shut down, restarted, or crashed. Closing a window does not Orphan anything. The `claude` process is dead, but the Worktree, Session, and log file are preserved. When the Host comes back, it surfaces its Orphans in the UI for the user to inspect, Resume, or Discard. Distinct from Fail (which exits abnormally *on its own*) and Stop (user-initiated).
_Avoid_: Abandoned, Dropped, Zombie.
