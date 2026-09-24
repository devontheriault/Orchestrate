# Handing an Agent's work to another Host

Since ADR 0009, an Agent belongs to one Host for its whole life, because its Session and its Worktree can't leave that machine. But the user sometimes wants the next prompt to run somewhere else, for example on the machine with the GPU or the one that has the database. So a stopped Agent's composer offers the other Hosts next to its own. Picking one **Hands off**: the prompt Spawns a new Agent on that Host, and that Agent starts from the old one's committed work.

The work travels the same way ADR 0011 moves all work between machines, **through the remote**:

- **The Agent's Host publishes.** It pushes the Agent's branch to the remote under a ref of its own, `cw/handoff/<id>`, never under the Agent's branch name. That way, taking the ref down later can't delete a branch the user pushed themselves. It also writes a brief: the old Agent's Task, the answer from its last Turn, and where its work started. Uncommitted work is refused, not left behind without a word. The user Commits first, as they would before a Merge.
- **The chosen Host picks it up.** It fetches the ref and cuts the new Agent's branch from it. It then deletes the ref from the remote, on a best-effort basis. The new Agent's Task is the brief followed by the user's prompt, so the transcript shows exactly what it was told. Its Base is the old Agent's Base, so its diff and its Merge cover the whole of the work, not only what it adds.

The old Agent is left as it was. It can still be replied to, Merged or Discarded on its own Host.

Considered and rejected:

- Moving the Agent itself by copying its Session and Worktree to the other machine. That breaks "an Agent lives on one Host", and Claude Code's Session files aren't ours to relocate.
- Pushing the Agent's own branch. That leaves a `cw/agent-*` branch on the remote with nothing to clean it up, and deleting it could delete one the user pushed on purpose.
- Committing loose work on the user's behalf. The app never Commits unasked.
