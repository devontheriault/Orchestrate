# Claude Code Control Layer

## Goal

Build a lightweight control layer for Claude Code that makes it easy to run, monitor, and coordinate multiple Claude Code agents across projects.

Claude Code remains responsible for coding and agentic work. This project manages the agents around it.

## Core Capabilities

- Spawn and manage Claude Code sessions
- Run agents in isolated Git worktrees
- Track projects, tasks, and agent status
- Monitor agent output and progress
- Pause, resume, and stop agents
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
6. Stop/pause/resume controls

Avoid building a custom agent loop or sophisticated multi-agent planning system initially.
