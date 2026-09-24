# Claude Code Control Layer

## Goal

Build a lightweight control layer for Claude Code that makes it easy to run, monitor, and coordinate multiple Claude Code agents across projects.

Claude Code remains responsible for coding and agentic work. This project manages the agents around it.

## Core Capabilities

- Spawn and manage Claude Code sessions
- Run agents in isolated Git worktrees
- Track projects, tasks, and agent status
- Monitor agent output and progress
- Resume and stop agents
- Support human approval at important points
- Coordinate multiple agents working toward the same goal

## Principles

- Keep the system simple.
- Prefer existing Claude Code capabilities over rebuilding them.
- Use Git worktrees for agent isolation.
- Don't build unnecessary abstractions.
- Human control should always be available.

## UI 
- The UI should be beautiful
- It should be intuitive
- It should be obvious and easy to use
- It should be multi-platform

## V1

Focus on:

1. Project management
2. Agent spawning
3. Git worktree isolation
4. Agent status/output
5. Basic task tracking
6. Stop/resume controls

Avoid building a custom agent loop or sophisticated multi-agent planning system initially.

## Code map

Conventions the directory tree won't tell you:

- `src/lib` is grouped by feature. Import across folders with `$lib/...`, within a folder with `./`.
- `src/lib/api.ts` is the IPC boundary only. Its types mirror the Rust structs (`domain.rs`, `git/`, `usage/`); change both together.
- App state lives in `src/lib/state/store.svelte.ts`. Slices that need the agents or the selection hang off it as `store.review`, `store.queue` and `store.prefs`, each in its own file. Standalone stores (`models`, `slash`, `usage`, `panes`, `theme`) are their own singletons.
- Pure logic sits in `.ts` beside the component that renders it (`transcript/rows.ts`, `agent/patch.ts`, `menus/menu.ts`). Put new pure logic there too, not in the component's script.
- Every drop-down menu uses `menus/menu.ts` for placement, dismissal and keyboard walking.
- Backend: `runtime/` owns live agents (`turn.rs` is one `claude` process), `git/` is split by user action, and `domain.rs` holds the persisted records. Tests for a directory module live in its `tests.rs`.

## Agent skills

### Issue tracker

GitHub Issues on `devontheriault/DevCode`, via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Default five-role vocabulary (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.
