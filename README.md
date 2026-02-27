# Legend

**Lightweight context memory for AI-assisted development.**

Legend persists project state and feature progress across sessions so AI coding assistants don't lose context. Run `legend init` in any project to get started.

## Installation

```bash
cargo install legend
```

## Quick Start

```bash
# Initialize Legend in your project
cd ~/my-project
legend init
```

This creates:
- `.legend/` - Legend state storage
- `.claude/settings.json` - Claude Code hooks (auto-loads context each session)

Now when you start Claude Code in this project, Legend context loads automatically.

## Usage

```bash
# View current state (human-readable)
legend show

# Get full state as JSON (for AI consumption)
legend get_state

# Search for features
legend search auth
legend search --status InProgress
legend search --domain api
legend search --tag backend

# Update features (pipe JSON to stdin)
echo '{"features": [{"id": "auth", "status": "Complete"}]}' | legend update

## Onboarding & Discovery

If you're starting in an existing codebase, Legend can autonomously generate an investigation plan to build a mental model of the project.

```bash
# 1. Scan the project to see a discovery report and investigation tasks
legend discover

# 2. Ingest high-signal files and the investigation plan into Legend's memory
legend discover --apply
```

### How LLM-Native Discovery Works:
1.  **Static Analysis:** Legend scans for manifests (`Cargo.toml`, `package.json`), entry points, and documentation.
2.  **Git Intelligence:** It analyzes the git history for architectural shifts, major refactors, and key decisions.
3.  **Investigation Tasks:** It generates a "Task List" for the AI assistant (e.g., "Investigate commit `a1b2c3d` regarding the new caching layer").
4.  **LLM Execution:** When you start a session, the LLM sees these tasks in memory and proactively runs the investigations to populate the long-term graph with high-signal insights.

## Tracking Features

Add a new feature:
```bash
echo '{
  "features": [{
    "id": "user-auth",
    "name": "User Authentication",
    "domain": "backend",
    "description": "Login/logout with JWT tokens",
    "status": "InProgress",
    "tags": ["security", "api"],
    "files_involved": ["src/auth.rs", "src/middleware.rs"]
  }]
}' | legend update
```

Update an existing feature (only `id` + changed fields needed):
```bash
echo '{"features": [{"id": "user-auth", "status": "Complete"}]}' | legend update
```

Remove a feature:
```bash
echo '{"remove_features": ["old-feature-id"]}' | legend update
```

## How It Works

Legend stores project state in `.legend/state.lz4` using bincode + LZ4 compression for fast (<5ms) reads. When you run `legend init`, it also creates Claude Code hooks that:

1. **SessionStart**: Automatically loads Legend context when you start Claude Code
2. **UserPromptSubmit**: Reminds Claude that Legend commands are available

This means Claude Code always knows about your project's features, their status, and which files are involved.

## Status Values

- `Pending` - Not started
- `InProgress` - Currently being worked on
- `Blocked` - Waiting on something
- `Complete` - Done

## License

MIT
