# Legend - Project Context Memory

This project uses **Legend**, a lightweight context memory tool for AI-assisted development. Legend tracks features, their status, and related files so you can pick up where you left off across sessions.

## Quick Reference

```bash
# Load full project state (JSON)
cargo run -- get_state

# Search for features by keyword
cargo run -- search <query>
cargo run -- search auth
cargo run -- search --domain cli
cargo run -- search --status Pending
cargo run -- search --tag backend

# View human-readable summary
cargo run -- show

# Update features (pipe JSON to stdin)
echo '{"features": [{"id": "my-feature", "status": "InProgress"}]}' | cargo run -- update
```

## When to Use Legend

### At Session Start
Run `cargo run -- get_state` to understand what features exist, their status, and what files they involve.

### When the User Mentions a Feature
If the user says something like "lets work on auth" or "fix the storage bug", search Legend first:
```bash
cargo run -- search auth
cargo run -- search storage
```
This gives you context about what's been done and which files are involved.

### After Completing Work
Update Legend to reflect what changed:
```bash
echo '{"features": [{"id": "feature-id", "status": "Complete", "files_involved": ["src/new_file.rs"]}]}' | cargo run -- update
```

### Adding New Features
When the user starts tracking something new:
```bash
echo '{"features": [{"id": "new-feature", "name": "Feature Name", "domain": "cli", "description": "What it does", "tags": ["relevant", "tags"]}]}' | cargo run -- update
```

## Update JSON Format

```json
{
  "features": [
    {
      "id": "required-unique-id",
      "name": "Required for new features",
      "domain": "Required for new features",
      "description": "Required for new features",
      "status": "Pending|InProgress|Blocked|Complete",
      "tags": ["optional", "labels"],
      "context": "Optional background info",
      "files_involved": ["src/relevant_file.rs"]
    }
  ],
  "remove_features": ["feature-id-to-delete"]
}
```

For existing features, only `id` plus changed fields are needed.

## Project Architecture

See PLAN.md for the full layer plan. Legend is built in Rust with:
- Bincode + LZ4 serialization (<5ms reads)
- Feature tracking with domain/tags/status filtering
- Exponential decay recency scoring (7-day half-life)

## Commit Style

Do not include `Co-Authored-By` lines in commits.
