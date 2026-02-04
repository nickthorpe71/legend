# Legend

**Lightweight context memory layer for AI-assisted development.**

Persists project state and feature progress across sessions so AI coding assistants don't lose context. Built in Rust following performance-first principles with <5ms operation targets.

> **Note:** This repository contains AI-generated code developed collaboratively with Claude Code as a learning project.

## Build

```bash
cargo build
cargo build --release  # Optimized binary
```

## Usage

```bash
legend help           # Show available commands
legend init           # Initialize .legend directory
legend get_state      # Get current state as JSON
legend update         # Update state from stdin
legend show           # Human-readable display
```

## Architecture

- **Storage Format**: Bincode (binary) + LZ4 compression for <5ms load times
- **State Files**: `.legend/state.lz4`, `.legend/config.toml`
- **Temporal Context**: Recency-weighted for intelligent retrieval
- **Performance**: <5ms per operation (see `PERFORMANCE.md`)

## Philosophy

- **Performance critical**: <5ms targets, continuous measurement
- **Minimal dependencies**: Only serde, bincode, lz4, serde_json
- **R* principles**: Clear, direct, adaptive (see `R*.md`)
- **Learning through building**: Production-quality code with teaching comments

See `CLAUDE.md` for development guidelines and `PLAN.md` for implementation roadmap.
