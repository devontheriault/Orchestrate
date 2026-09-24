# Tailscale identity is how a Host trusts a window

A Host accepts connections that spawn YOLO Agents running as the user, so reaching one is close to running code on that machine. Hosts are reached **over Tailscale only**: a Host listens on its tailnet address, never on every interface, and is added in a window by its Tailscale name.

Being on the tailnet is not enough on its own — nodes shared into it, or any compromised device on it, could reach the Host. So on each connection the Host asks Tailscale who the peer is and **accepts only the user's own Tailscale identity**. There is no pairing code or token: Tailscale already authenticates every device, and a second secret to copy between machines would add a step without closing any gap the identity check leaves open. The connection from a window to its own machine's Host is local and needs no check.

Agents run as the user's own account, with their own Claude Code login, skills and settings, as they always have. A dedicated user or container would limit what a YOLO Agent can reach, but costs a separate login and loses the user's own configuration; it can be added later as a deployment change without changing this design.

Considered and rejected: a Host name alone (trusts every device on the tailnet), and a per-window pairing token (a second credential for the same question Tailscale already answers).
