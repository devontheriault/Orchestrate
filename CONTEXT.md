# Claude Code Control Layer

A lightweight desktop app that spawns, monitors, and controls Claude Code agents running in isolated Git worktrees. This project manages the agents around Claude Code, not the coding work itself.

## Language

**Agent**:
One `claude` process, bound to one Worktree, running one Task. Its lifetime is the process's lifetime; when the process ends, the Agent ends.
_Avoid_: Session (has a different, later meaning), Runner, Job, Worker.

**Task**:
The prompt or work item a user hands an Agent when spawning it. In V1, an Agent runs exactly one Task.
_Avoid_: Job, Prompt (too narrow — the Task is the *unit of work*, not just the string), Instruction.

**Project**:
A local Git repository the user has explicitly registered with the app. A Project may contain many Agents over time (spawned across separate Worktrees), and the Project's own working tree is never touched by an Agent.
_Avoid_: Workspace, Repo (in UI), Directory.

**Worktree**:
An isolated Git worktree created for one Agent, living under a global scratch directory outside the Project. The Worktree is the Agent's sandbox; it is reaped when the Agent stops.
_Avoid_: Checkout, Clone, Branch dir.

**Session**:
Reserved. Not a V1 concept. When resumable Claude Code conversations (via `claude --resume`) become a feature, "Session" will name that concept. Do not use it as a synonym for Agent.

## Agent lifecycle

An Agent moves through these states, and each transition has a specific verb.

**Spawn**:
The transition that creates a new Agent: allocate a Worktree, start the `claude` subprocess, wire up streaming. After Spawn, the Agent is *running*.

**Stop**:
A user-initiated destructive exit. The `claude` process is terminated and the Worktree is Reaped in the same action. Any in-progress work is discarded. Contrast with Complete.
_Avoid_: Kill, Cancel, Abort.

**Complete**:
The Agent's natural exit — the `claude` process exits on its own after finishing its Task. The Worktree is *preserved* so the user can inspect the diff and merge or copy work out. Distinct from Stop: Complete is constructive, Stop is destructive.
_Avoid_: Finish, End, Done (as verbs).

**Fail**:
The Agent's unnatural exit — the `claude` process exits non-zero, is killed by the OS, or otherwise crashes. Distinct from Complete (natural exit) and Stop (user-initiated). Like Complete, the Worktree is preserved for post-mortem inspection; the user must explicitly Reap when done. The last JSONL events and exit code are recorded.
_Avoid_: Crash, Error (as state names).

**Reap**:
Destroys an Agent's Worktree and removes it from the app's active list. Reap is automatic on Stop; on Complete or Fail it is an explicit user action after review. After Reap, the Agent's on-disk log file (JSONL) is still preserved.
_Avoid_: Cleanup, Delete, Remove.

**Orphan**:
An Agent that was running when the app was closed. The `claude` process is dead (SIGKILL on close), but the Worktree and log file are preserved. On next launch, the app surfaces Orphans in the UI for the user to inspect or Reap. Distinct from Fail (which exits abnormally *on its own*) and Stop (user-initiated).
_Avoid_: Abandoned, Dropped, Zombie.
