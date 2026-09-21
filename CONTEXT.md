# Claude Code Control Layer

A lightweight desktop app that spawns, monitors, and controls Claude Code agents running in isolated Git worktrees. This project manages the agents around Claude Code, not the coding work itself.

## Language

**Agent**:
One `claude` process, bound to one Worktree, running one Task. Its lifetime is the process's lifetime; when the process ends, the Agent ends.
_Avoid_: Session (has a different, later meaning), Runner, Job, Worker.

**Task**:
The work item a user hands an Agent at Spawn — its opening prompt. An Agent has exactly one Task for its whole life; follow-ups refine it rather than replace it. The Task is what the Agent was asked to do, not what the UI calls it — that's the Title.
_Avoid_: Job, Prompt (too narrow — the Task is the *unit of work*, not just the string), Instruction.

**Title**:
The short name the UI identifies an Agent by, written by Claude from the Agent's own work: at the end of a Turn we hand a cheap, isolated `claude` the Task and the Turn's answer and ask for a few words. Written at the end of each of the first three Turns and then frozen — a name that kept shifting would cost the user the thing a name is for, which is finding the same Agent again tomorrow. Until the first Turn ends, and on Agents recorded before Titles existed, the first line of the Task stands in. Ours rather than Claude Code's own `ai-title`: that one is only written for interactive sessions, and it is shared across the sessions in a directory, so sibling Agents would all wear the same name.
_Avoid_: Name (too close to the Project's), Summary, Label, Description.

**Turn**:
One prompt-and-answer exchange within an Agent: one `claude` process, from Spawn or Resume until it exits. Turn 1 carries the Task; later Turns carry follow-up prompts. Turns are counted on the Agent and recorded in its log, so the output pane reads as a conversation.
_Avoid_: Run, Iteration, Round, Message (a Turn contains many messages).

**Queue**:
What the user has said to an Agent that wasn't free to hear it yet: prompts typed while a Turn was still running, held in order and sent as their own Turns once the Agent is free. A Queue belongs to one Agent, is always visible above the composer with its messages readable and individually removable, and drains one message per clean Complete — a Stop or a Fail holds the rest, since interrupting an Agent shouldn't fire the rest of the line into it. Kept in the window's local storage rather than on the Agent: it is a record of what the user means to say, not part of the conversation Claude Code owns.
_Avoid_: Buffer, Pipeline, Inbox.

**Project**:
A local Git repository the user has explicitly registered with the app. A Project may contain many Agents over time (spawned across separate Worktrees), and no Agent ever touches the Project's own working tree. The app touches the Project only when the user asks it to: a Land advances one of the Project's branches, and reaches its working tree only when the branch being landed onto is the one checked out.
_Avoid_: Workspace, Repo (in UI), Directory.

**Worktree**:
An isolated Git worktree created for one Agent, living under a global scratch directory outside the Project. The Worktree is the Agent's sandbox; it is reaped when the Agent stops.
_Avoid_: Checkout, Clone, Branch dir.

**Base**:
The commit an Agent's branch was cut from at Spawn, recorded on the Agent. Everything the Agent produced is expressed as a diff against its Base, so committed and uncommitted work read as one change set even if the Project's own branch moves on afterwards.
_Avoid_: Parent, Fork point, Origin.

**Model**:
Which Claude model an Agent's Turns run on, recorded on the Agent and passed to `claude --model` as a full model name (`claude-opus-4-5-20251101`), version included, so an Agent keeps running on the model the user actually picked. The choices come from Anthropic's Models API, asked with the same credential `claude` itself uses — the list is the user's own account, not one baked into this app, so it covers models released after any given build and never offers one the Agent couldn't run. Picked at Spawn and changeable at Resume: starting cheap and escalating is a normal move, so the Model belongs to the Turn as much as to the Agent. Unset means we pass no `--model` at all and Claude Code's own configured default applies; that is also the fallback when the account can't be reached, so the picker always has a working choice. The last Model the user picked for any Turn is remembered in the window's local storage and is what the next Spawn opens on — people work on one model for stretches at a time, so re-picking it every Spawn is friction; on a profile that has no record yet, the most recently spawned Agent's Model stands in.
_Avoid_: Engine, Backend, Tier.

**Effort**:
How hard an Agent's Turns think, passed to `claude --effort` as one of `low`, `medium`, `high`, `xhigh`, or `max`. It rides with the Model rather than claiming a control of its own — it is picked from a submenu hanging off each model row, because how capable a model is and how hard it works are one decision about what a Turn is worth. Like the Model it is picked at Spawn, changeable at Resume, carried on each queued message as it stood when that message was queued, and remembered in the window's local storage, falling back to the most recently spawned Agent's Effort on a profile with no record. Unset means we pass no `--effort` at all and Claude Code's own level applies; that is also what Agents recorded before Effort existed get.
_Avoid_: Thinking budget, Reasoning level, Depth, Quality.

**Permission Mode**:
What an Agent's Turns may do without asking, passed to `claude --permission-mode`. Two choices: `bypassPermissions`, the default, where the Agent acts freely inside its Worktree, and `plan`, where it reads and proposes but writes nothing. The modes Claude Code offers that stop to ask are deliberately absent — we hand `claude` no input channel, only `--output-format stream-json`, so a permission prompt would hang the Turn with nowhere to answer it. Isolation rather than permission is the safety story here: the Worktree is the sandbox, which is why acting freely inside one is the default, and `plan` exists for the Agent you want to think before it touches anything. Picked at Spawn, changeable at Resume, and carried on each queued message as it stood when that message was queued, exactly as the Model and Effort are. A plan-mode Agent still gets a Worktree and still reads the Project's code; it simply produces nothing to Commit or Land, and the UI says so rather than showing an empty diff.
_Avoid_: Mode (too vague on its own), Sandbox (that's the Worktree), Approval, Safety level.

**Session**:
The Claude Code conversation history behind an Agent, named by the UUID we mint at Spawn and pass as `--session-id`. Claude Code owns the transcript; we only keep the ID, and Resume hands it back via `--resume`. A Session is scoped to the directory it started in, which is why it survives exactly as long as the Agent's Worktree does.
_Avoid_: using it as a synonym for Agent (an Agent is the thing the user talks to; the Session is the history that makes talking again possible), Thread, History, Context.

**Usage**:
What Turns have cost: tokens and dollars per Model, plus how much of the account's rate-limit windows is spent. Read back out of the Agent logs rather than tallied as events arrive — Claude Code reports a Turn's per-model totals on its `result` event and the account's windows on `rate_limit_event`, so the logs are the record and the numbers are right after a restart. Account-wide rather than per-Project: every Agent spends against the same limits, which is why the usage window totals across Agents by default, and why a log that outlived its Reaped Agent still counts toward the total.
_Avoid_: Cost (only half of it), Quota, Budget (nothing here enforces one), Stats.

## Agent lifecycle

An Agent moves through these states, and each transition has a specific verb.

**Spawn**:
The transition that creates a new Agent: allocate a Worktree, mint a Session ID, start the `claude` subprocess, wire up streaming. After Spawn, the Agent is *running* its first Turn.

**Resume**:
Putting a stopped Agent back to work with a follow-up prompt: start a new `claude` in the Agent's existing Worktree with `--resume <session>`, and return the Agent to *running* for another Turn. Available from Completed, Failed, Stopped, and Orphaned — anything with a Session and a Worktree still on disk. Refused while the Agent is already working; Stop it first to change course. Resume is how a user keeps talking to one Agent instead of Spawning another.
_Avoid_: Continue, Restart (a Resume keeps the conversation; a restart would discard it), Retry, Follow-up (as a verb).

**Stop**:
A user-initiated end to the current Turn. The `claude` process is terminated (SIGTERM, then SIGKILL after a grace period) and the Agent goes to *stopped*. The Worktree and Session are preserved, so a Stopped Agent can be inspected, Committed, or Resumed with a corrected prompt — Stop is how a user interrupts an Agent heading the wrong way. Only Reap destroys anything.
_Avoid_: Kill, Cancel, Abort.

**Complete**:
A Turn's natural end — the `claude` process exits zero on its own. The Worktree is preserved so the user can inspect the diff and merge or copy work out, and the Session is preserved so the user can Resume with a follow-up. The common case is an Agent that sits Completed between Turns for as long as the user wants.
_Avoid_: Finish, End, Done (as verbs).

**Fail**:
A Turn's unnatural end — the `claude` process exits non-zero, cannot be started at all, is killed by the OS, or otherwise crashes. Distinct from Complete (clean exit) and Stop (user-initiated). Like Complete, the Worktree and Session are preserved: the user can read the stderr in `fail_reason`, then Reap or Resume. The last JSONL events and exit code are recorded.
_Avoid_: Crash, Error (as state names).

**Commit**:
A user action on a Completed, Failed, Stopped, or Orphaned Agent: stage everything in its Worktree and commit it onto the Agent's own branch. This is how work survives a later Reap — the commit stays reachable in the Project's object store — and it is what a Land needs, since only committed work can be landed. Refused while the Agent is *running*, since it would capture a tree the Agent is still writing. The app never commits on its own; whether an Agent commits its own work is up to Claude Code.
_Avoid_: Save, Merge (that's Land — a Commit stays on the Agent's own branch), Checkpoint.

**Land**:
Merging a Committed Agent's branch into a branch of the user's choosing — the only action that writes to the Project, and never one an Agent takes. The target is picked from the Project's local branches, its checked-out branch first and by default, with `cw/agent-*` branches excluded: landing one Agent onto another's branch is the multi-agent coordination V1 defers. The merge is always `--no-ff`, so the Agent's work stays identifiable as one unit in history — that is the point of giving each Agent its own branch, and squashing or rebasing would break the promise that everything an Agent produced reads as one diff against its Base. When the target is not the checked-out branch the merge happens in a throwaway worktree, so a Land never moves the user off their branch; when it is, the merge happens in place and is refused against a dirty tree. Refused also while the Agent is working, and while its Worktree holds uncommitted work — Commit first. A conflict aborts the merge, leaving the Project byte-identical and naming the files that collided: this app has no merge tool, and a half-merged Project is somewhere the app could not get it back out of. A Land does not end an Agent; it is recorded on the Agent, with the branch it landed on, the way the Base is.
_Avoid_: Merge (the git operation is how a Land is done, not what it means), Ship, Integrate, Promote.

**Reap**:
Ends an Agent for good: destroys its Worktree, deletes its branch, and removes it from the app's list. Always an explicit user action, and the only action that destroys anything — so it discards Commits the Agent made as well as uncommitted work, and takes the Session with it (a Session cannot outlive the directory it ran in). Refused while the Agent is working. After Reap, the Agent's on-disk log file (JSONL) is still preserved.
_Avoid_: Cleanup, Delete, Remove.

**Orphan**:
An Agent that was working when the app was closed. The `claude` process is dead (SIGKILL on close), but the Worktree, Session, and log file are preserved. On next launch, the app surfaces Orphans in the UI for the user to inspect, Resume, or Reap. Distinct from Fail (which exits abnormally *on its own*) and Stop (user-initiated).
_Avoid_: Abandoned, Dropped, Zombie.
