# Claude Code Control Layer

A lightweight desktop app that spawns, monitors, and controls Claude Code agents running in isolated Git worktrees. This project manages the agents around Claude Code, not the coding work itself.

## Language

**Agent**:
One `claude` process, bound to one Worktree, running one Task. Its lifetime is the process's lifetime; when the process ends, the Agent ends.
_Avoid_: Session (has a different, later meaning), Runner, Job, Worker.

**Task**:
The work item a user hands an Agent at Spawn — its opening prompt, and the label the UI identifies the Agent by. An Agent has exactly one Task for its whole life; follow-ups refine it rather than replace it.
_Avoid_: Job, Prompt (too narrow — the Task is the *unit of work*, not just the string), Instruction.

**Turn**:
One prompt-and-answer exchange within an Agent: one `claude` process, from Spawn or Resume until it exits. Turn 1 carries the Task; later Turns carry follow-up prompts. Turns are counted on the Agent and recorded in its log, so the output pane reads as a conversation.
_Avoid_: Run, Iteration, Round, Message (a Turn contains many messages).

**Project**:
A local Git repository the user has explicitly registered with the app. A Project may contain many Agents over time (spawned across separate Worktrees), and the Project's own working tree is never touched by an Agent.
_Avoid_: Workspace, Repo (in UI), Directory.

**Worktree**:
An isolated Git worktree created for one Agent, living under a global scratch directory outside the Project. The Worktree is the Agent's sandbox; it is reaped when the Agent stops.
_Avoid_: Checkout, Clone, Branch dir.

**Base**:
The commit an Agent's branch was cut from at Spawn, recorded on the Agent. Everything the Agent produced is expressed as a diff against its Base, so committed and uncommitted work read as one change set even if the Project's own branch moves on afterwards.
_Avoid_: Parent, Fork point, Origin.

**Model**:
Which Claude model an Agent's Turns run on, recorded on the Agent and passed to `claude --model` as a full model name (`claude-opus-4-5-20251101`), version included, so an Agent keeps running on the model the user actually picked. The choices come from Anthropic's Models API, asked with the same credential `claude` itself uses — the list is the user's own account, not one baked into this app, so it covers models released after any given build and never offers one the Agent couldn't run. Picked at Spawn and changeable at Resume: starting cheap and escalating is a normal move, so the Model belongs to the Turn as much as to the Agent. Unset means we pass no `--model` at all and Claude Code's own configured default applies; that is also the fallback when the account can't be reached, so the picker always has a working choice.
_Avoid_: Engine, Backend, Tier.

**Session**:
The Claude Code conversation history behind an Agent, named by the UUID we mint at Spawn and pass as `--session-id`. Claude Code owns the transcript; we only keep the ID, and Resume hands it back via `--resume`. A Session is scoped to the directory it started in, which is why it survives exactly as long as the Agent's Worktree does.
_Avoid_: using it as a synonym for Agent (an Agent is the thing the user talks to; the Session is the history that makes talking again possible), Thread, History, Context.

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
A user action on a Completed, Failed, Stopped, or Orphaned Agent: stage everything in its Worktree and commit it onto the Agent's own branch. This is how work survives a later Reap — the commit stays reachable in the Project's object store. Refused while the Agent is *running*, since it would capture a tree the Agent is still writing. The app never commits on its own; whether an Agent commits its own work is up to Claude Code.
_Avoid_: Save, Merge (Commit does not touch the Project's branch), Checkpoint.

**Reap**:
Ends an Agent for good: destroys its Worktree, deletes its branch, and removes it from the app's list. Always an explicit user action, and the only action that destroys anything — so it discards Commits the Agent made as well as uncommitted work, and takes the Session with it (a Session cannot outlive the directory it ran in). Refused while the Agent is working. After Reap, the Agent's on-disk log file (JSONL) is still preserved.
_Avoid_: Cleanup, Delete, Remove.

**Orphan**:
An Agent that was working when the app was closed. The `claude` process is dead (SIGKILL on close), but the Worktree, Session, and log file are preserved. On next launch, the app surfaces Orphans in the UI for the user to inspect, Resume, or Reap. Distinct from Fail (which exits abnormally *on its own*) and Stop (user-initiated).
_Avoid_: Abandoned, Dropped, Zombie.
