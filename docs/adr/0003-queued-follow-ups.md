# Queued Follow-ups

_Where the Queue lives is superseded by ADR 0010: it moved from the window to the Host._

ADR-0002 rejected queueing a follow-up typed while an Agent is still working, on two grounds: sending it as a second concurrent `claude` in one Worktree would have two processes writing the same files, and "holding it to send later hides a pending action in the UI". The composer was disabled mid-Turn, and the only way to say anything was to Stop first.

That second ground turns out to be an argument about the UI, not about queueing. A Queue that is *visible* — a strip directly above the composer that says how many messages are waiting, opens to show each prompt, and lets any of them be removed — hides nothing. Meanwhile the first ground is satisfied by never sending a queued message while a Turn is in flight: the Queue drains into a **new Turn** on an Agent that is already free, which is exactly what Resume has always done. One `claude` per Worktree still holds.

The cost of the old rule was real. An Agent working for several minutes is the normal case, and the user's next instruction usually arrives mid-Turn. Their choices were to Stop an Agent that was doing fine, or to keep the thought in their head until the Turn ended and watch for it. So:

- A prompt typed while an Agent is working is **queued**, not refused; **Enter** queues it, and the placeholder says so — "Working… press Enter to queue a message". There is no queue button.
- **Stop takes the send button's place.** While an Agent works it occupies the box's bottom-right, the slot the user already reads as *the* button, so interrupting is one click from where they are typing. The corner holds exactly one button at any moment: Send when the Agent is free, Stop when it isn't. An earlier revision put an outlined queue button beside Stop, but two lookalike icon buttons in that corner meaning opposite things — end the Turn, hold a message — read as clutter and invited the wrong click. The corner is worth more as one unambiguous control than as a second home for queueing, which Enter already does.
- A Queue **drains on Complete only**. The `agent-state-changed` listener sends the head of the Queue when a Turn ends cleanly. A Stop or a Fail holds it: the main reason to Stop is that the Agent is going the wrong way, and auto-sending three more messages into it would undo the interrupt. A held Queue says so and offers "Send next".
- Each queued message **carries the Model and effort** picked when it was queued. The pickers stay live mid-Turn, so queueing a cheap follow-up behind an expensive Turn works without waiting to re-pick.
- The Queue lives in the **window's local storage**, keyed by Agent id, and is pruned on load of any Agent that no longer exists. A reload or a relaunch shouldn't silently discard text the user typed, and Claude Code owns the transcript but has no idea about messages that were never sent. A Discard takes the Agent's Queue with it.

Considered and rejected: sending queued messages after a Fail as well (a failed Turn is a thing to read, not to talk over), editing a queued message in place (deleting and retyping is enough for a message that is usually one line), and a backend Queue (nothing here needs to outlive the window, and a Queue the app can't see is the hidden state ADR-0002 was right to object to).
