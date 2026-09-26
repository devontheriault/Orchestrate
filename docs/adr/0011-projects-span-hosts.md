# Projects span Hosts, and the remote is where work lands

With several Hosts, the same repository is checked out on more than one machine. We treat those checkouts as **one Project, identified by its remote**: its Agents sit together under one heading whichever Host they run on, and the Host is picked at Spawn (by default the one last used for that Project). A Project with no remote exists on one Host only. The alternative, one Project per Host, is simpler to store but shows the same repository several times and makes "where it runs" the first thing the user decides instead of a detail.

Any Host can take any remote-backed Project without setup: **if a Host has no checkout, it clones one** into a folder it manages. A checkout the user registered on that Host themselves takes precedence, and registering one later takes over from the clone for new Agents. Trusting a Project is asked once per Project, not per Host, since the repository is what is being trusted.

Because an Agent's work lives on its Host, **the remote is the source of truth** for getting it anywhere else:

- **Spawn fetches first**, then cuts from whichever of the local branch and its upstream is newer when one contains the other. On divergence it cuts from the local branch — the user sitting at that machine may have unpushed work they mean the Agent to have — and the transcript says what that leaves out. The Project's checkout is never moved.
- **Merge fetches, merges, and pushes the target.** A target only behind its upstream is fast-forwarded first unless it is checked out with local changes; one that has diverged is refused and named, as a conflict is. A rejected push leaves the Merge in place, recorded as not yet pushed, and offers the push again. Pushing unasked is a write beyond the Project, but a Merge the user can't see from their other machines is not what they asked for. *(Superseded by ADR 0014: a Merge pushes only when the user asks.)*

Removing a Project unregisters it on every reachable Host and names the ones it couldn't reach; as before, Agents and checkouts stay until Discarded.

Considered and rejected: making Merge local-only with a separate Push, which leaves the user's other machines behind by default; and replacing Merge with pushing the Agent's branch or opening a PR, which drops the `--no-ff` Merge ADR 0004 built the Delivered story on.
