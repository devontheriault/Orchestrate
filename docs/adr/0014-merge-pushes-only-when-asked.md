# A Merge pushes only when asked

ADR 0011 had every Merge push its target, so the user's other machines never fell behind. In use, that made pushing the default for a write that leaves the machine and can't be taken back, and the user asked for it the other way round: **a Merge lands locally, and pushes only if the user turns Push on for it.** The Merge control offers a Push switch beside the branch picker, off each time, and only for a target that tracks a remote.

A Merge into a tracked target that wasn't pushed is recorded on the Agent as not yet pushed (`unpushed`), and the Diff tab offers Push, as it already did after a failed push; `/push` does the same from the composer. A failed push is still recorded with its reason and shown as a warning. Not pushing is the default, so it gets no warning. A Resolver carries the choice made on the Merge that conflicted, and pushes when it finishes only if that Merge was to.

Everything else in ADR 0011 stands: a Merge still fetches first, fast-forwards a target that is only behind, and refuses one that has diverged. Those steps only read the remote.
