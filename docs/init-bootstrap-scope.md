# Init bootstrap scanning scope (#25)

**Recorded:** 2026-04-24

Closes queue item #25: "Review init bootstrap scanning scope". The
audit covers what `legend init` (via `discover::run_discovery` →
`bootstrap_keywords_from_workspace`) reads and what it seeds.

## Scope today

### Walker (`walk_directory`)

- Visits every file under the workspace root, recursively.
- Skips hidden directories (any name starting with `.`).
- Skips a fixed allowlist of build/dependency directories: `target`,
  `node_modules`, `build`, `bin`, `dist`, `out`, `coverage`,
  `htmlcov`, `__pycache__`, `vendor`.
- Records the file path and its extension counter.
- **Does not** read file content.

### High-signal classifier (`scan_high_signal`)

For each walked file, classifies into one of three buckets:

| Kind          | Match rule                                         |
|---------------|----------------------------------------------------|
| Documentation | `README.md`, `ARCHITECTURE.md`, `VISION.md`, `PLAN.md`, `GEMINI.md`, `CLAUDE.md`, `CODEX.md`, `PRD.md` (root-level), or any subdirectory `README.md` |
| Manifest      | `Cargo.toml`, `package.json`, `go.mod`, `requirements.txt`, `pyproject.toml`, `Pipfile`, `Gemfile`, `Makefile`, `composer.json`, `build.gradle`, `pom.xml`, `deno.json` |
| EntryPoint    | `main.rs`, `lib.rs`, `main.py`, `app.py`, `index.ts`, `index.tsx`, `index.js`, `App.tsx`, `App.jsx`, `server.js`, `main.go` |

### Bootstrap keyword seeding

Reads the high-signal files (only) and seeds `kw:<category>:<term>`
nodes:

| Source                       | Category    | Notes |
|------------------------------|-------------|-------|
| Manifest dependency names    | tool        | Cargo.toml [dependencies], package.json deps, etc. |
| Detected tech_stack          | tool        | Same lookup, normalized |
| Documentation headings       | architecture | Per-line `#` extraction |
| Documentation recurring terms| domain      | Frequency-based |
| Manifest config keys         | environment | E.g. `[features]` keys, `scripts` |
| Entry-point identifiers      | domain      | Top-level fn/struct/type names |
| Per-language keyword sets    | code        | Language-specific keyword lists |

## Audit findings (this session)

### Applied

1. **SKIP_DIRS**: removed redundant hidden-prefix entries (`.git`,
   `.legend`, `.vscode`, `.idea`) — already covered by the
   `starts_with('.')` walker rule. Added `dist`, `out`, `coverage`,
   `htmlcov`, `__pycache__`, `vendor`, all common in production
   monorepos.
2. **Manifests**: added `Pipfile`, `composer.json`, `build.gradle`,
   `pom.xml`, `deno.json`. The previous list missed all JVM and PHP
   ecosystems and the modern Python `Pipfile`.
3. **Entry points**: added `index.tsx`, `App.tsx`, `App.jsx`,
   `server.js`. React projects use `App.tsx` more than `index.tsx`;
   Node.js services use `server.js`.

### Deferred (open questions, not changed)

1. **Subdirectory CLAUDE.md / ARCHITECTURE.md** are silently ignored.
   Only root-level docs and subdirectory `README.md` are picked up.
   Worth lifting once we know the convention has stabilized.
2. **`.gitignore` parsing** is not done. The walker uses an inlined
   skip list instead. Adding `gitignore` crate would honor user
   overrides at the cost of a new dependency. Nice-to-have.
3. **Symlink loop guard**. `fs::read_dir` follows symlinks; a
   self-referential symlink would recurse forever. Not observed in
   practice but worth a `metadata.is_symlink()` check.
4. **Recursion depth limit**. None today. Stack-blow-up risk only on
   pathological depth (>1000 dirs). Cap at, say, 32 if we ever see it.
5. **Doc heading + recurring-term extraction is permissive.** Future
   work: salience-weight extracted terms before seeding so a one-off
   typo in a README doesn't become a learned `kw:domain:` node.
6. **No file-size cap on high-signal reads**. Today every matched
   manifest/doc is read fully. A 10 MB minified bundle masquerading
   as `README.md` would block init briefly. Cap at, say, 256 KB.

## Why "review and not rewrite"

Init runs once per project and isn't on the hot path. The current
scope produces useful seeded keywords without false-positive noise on
the workspaces tested in `tests/conformance_discover.rs`. The applied
changes above are low-risk additions that don't change shape; the
deferred items are either nice-to-have safety guards or quality
investments that need real data (large project corpora) to evaluate.

## Related

- `docs/chunking-evaluation.md` (#19): extraction is the bottleneck
  for runtime ticks; bootstrap seeding is a separate, init-time path.
- Commit `81e45bc` removed fixture-specific keyword shortcuts; the
  general bootstrap path stays as the only seed mechanism.
- Queue item #26 (`Decide init re-run behavior on already-initialized
  repo`) is the natural follow-on — it picks up the lifecycle question
  this audit deliberately avoids.
