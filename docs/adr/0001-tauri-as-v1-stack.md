# Tauri as the V1 stack

V1 is a portable desktop app (Linux-first, must reach macOS/Windows) that spawns Claude Code as a subprocess and streams its output. We picked **Tauri** (Rust core + web frontend) over Electron, Wails, Flutter Desktop, and native Rust GUI toolkits.

Rust is a strong fit for the wrapper's core work — process spawning, stdout parsing, filesystem/worktree management, event streaming — and Tauri's system-webview approach keeps bundles small and feel native. A web frontend keeps UI iteration fast, which the "beautiful, intuitive" UI bar demands. Electron would have worked but its bundle bloat conflicts with the "lightweight" principle; Wails and Flutter have smaller ecosystems; native Rust GUI is younger and slower to iterate on.

Considered and rejected: routing Claude Code through the Claude Agent SDK (Python/TS). Under Tauri that would force a sidecar architecture and IPC boundary, which negates the SDK's typed-events benefit. The CLI's `--output-format stream-json` gives the same structured events; a Rust type layer over those events buys back the SDK's ergonomics without leaving Rust.
