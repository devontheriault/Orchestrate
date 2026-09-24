# The Queue lives on the Host

ADR 0003 kept the Queue in the window's local storage and rejected a backend Queue because "nothing here needs to outlive the window". Agents that work while no window is open (ADR 0009) make that false: a Queue drained by the window stops draining when the laptop lid closes, which is exactly when an unattended Agent needs its next message. Two windows would also each hold a Queue of their own and could both send the same message. So the Queue moves onto the Agent's Host.

**The Host decides whether a message starts a Turn or is queued.** The window only ever sends; the Host starts a Turn if the Agent is free and appends to its Queue if not. That makes "one `claude` per Worktree" a rule enforced in one place, so two windows sending at the same moment cannot produce two Turns.

Everything else in ADR 0003 stands: the Queue is always visible and editable in every window, drains only on a clean Complete, is held by a Stop or a Fail, and each message keeps the Model, Effort and Permission Mode it was queued with. ADR 0002's objection was to hidden state, not to state kept on the backend, and a Queue every window shows is not hidden.

Queues found in a window's local storage are handed to the Host the first time that window connects, then cleared there.
