# Production roadmap — deploy Legend across real projects with analytics

**Goal:** move Legend from a single-project trial (alchamancer2) to a version you can
drop into real projects on multiple machines, keep recording per-project analytics, and
periodically gather + analyze across all of them.

**Decisions (2026-07-27):**
- **Analytics: pull-based aggregator.** Each project keeps its own journal (already the
  design); a tool scans a set of `.legend` dirs on demand and rolls up metrics. No
  server, data stays local. Cross-machine gathering rides the fact that each journal is
  git-committed in its project repo — pull the repos, run the aggregator.
- **First build: install + deploy docs** (Linux/WSL, unblocked today).
- **Topology: multiple machines.**
- **Platforms: Linux (this), WSL (= Linux, free), native Windows (a real port).**

## What already works ✓
- **Per-project setup:** `legend init` writes `.mcp.json`, `.claude/settings.json` (the
  three hooks), and `AGENTS.md` (Codex) — generic, not alchamancer-specific.
- **Per-project analytics:** the journal (`.legend/journal.jsonl`), one verbatim,
  build-stamped, replayable line per invocation. This *is* the analytics stream.
- **Runtime:** single-file C, no deps beyond libm; deterministic; fuzz-tested; embed-off
  fallback if the model is missing.
- **Per-project analysis:** `harness/round_report.py` (activity, pollution rates,
  retrieval quality via ambient replay, invariants).

## Workstreams

### W1 — Cross-platform portability (Linux ✓ · WSL ✓ · Windows: port)
WSL runs the Linux build unchanged. Native Windows needs a small platform-shim layer
(`#ifdef _WIN32`) for the audited Unix-isms — bounded, ~6 shims, not a rewrite:
- `readlink("/proc/self/exe")` → `GetModuleFileNameA` (binary-relative model dir;
  `embed.c:463`, `legend.c:8926`)
- `flock(LOCK_EX)` store lock → `LockFileEx` / `_locking` (`legend.c:8727-8756`)
- `dirent`/`readdir` snapshot-dir scan → `FindFirstFile`/`FindNextFile` (`legend.c:8251`)
- `signal(SIGPIPE, SIG_IGN)` → no-op on Windows (`legend.c:10445`)
- `mkdir(mode)` / `struct stat` → `_mkdir` / `_stat` guards
- POSIX headers (`unistd.h`, `sys/file.h`, `dirent.h`, `fcntl.h`) → guarded includes
- Paths: forward slashes already work in Win32 fopen; audit `snprintf("%s/%s")` sites.
- **Toolchain:** MinGW-w64 or clang (C99-clean); MSVC is possible but has C99 quirks.
- **Hooks on Windows (open):** the `settings.json` hooks are bash. How Claude Code runs
  hooks on native Windows (Git Bash? cmd/powershell?) determines whether they port as-is
  or need shell variants. WSL uses bash → fine. **Resolve before claiming Windows-native
  hook support.**

### W2 — Build & install (per platform)
- **Linux/WSL (first):** a `Makefile` — `make` (strict, sha-stamped build), `make install`
  (binary → `~/.local/bin`, model `minilm.int8.bin`+`vocab.txt` → `~/.local/share/legend/…`).
  Only those two model files are needed by the C embedder (the 161M `models/` dir also
  holds `model.safetensors` etc. for other tooling — do NOT ship those).
- **Windows:** after W1 — a build script (MinGW/clang) + an install that places the
  binary on PATH and the model beside it. Consider CMake if the per-platform build
  scripts multiply.
- **Model distribution:** ~34MB (`minilm.int8.bin` 34M + `vocab.txt` 231K), committed in
  git, so `clone → make install` carries it. (Optional later: a fetch-from-URL path to
  keep clones small.)

### W3 — Baseline versioning
Freeze/tag a stable build as the production baseline so every deployment is
version-comparable. The journal already stamps the build sha per line, so the aggregator
flags drift. Tag current HEAD once W1/W2 land (or now for Linux/WSL).

### W4 — Multi-project analytics aggregator (the key new tool)
Generalize `round_report.py` → an aggregator that takes a **set** of `.legend` dirs (or a
registry file listing them), computes the per-project metrics on each, and emits a
**cross-project rollup + comparison**: usage volume, save/recall mix, pollution rates
(prose_name/bloat/etc. normalized per-1k-elements), retrieval quality (ambient surface
rate), rejections/errors, latency, growth, build-version spread. Cross-machine: run it
where the project repos are checked out (journals are committed), or after a `git pull`
sweep. Output: a table + per-project cards; optionally an HTML/artifact report.

### W5 — Onboarding + docs
- **Getting-started / deploy guide:** `init → onboard → use`, per platform.
- **Onboarding, generalized:** define what a *new* project ingests on the first deep
  session (docs, module tree, history) — the plumbing (`init`) exists; the first-session
  ingest recipe needs to be generic + documented.
- **"Read your analytics" guide:** how to run the aggregator and interpret it.

### W6 — Production hardening (as-needed, not blocking)
- Periodic health-check / `maintain` surface (pollution accrues; today it's manual).
- Failure-mode audit for unattended use (missing model → embed-off; corrupt snapshot →
  handled; concurrent access → per-call lock).

## Critical path & sequencing
1. **Linux/WSL install + deploy docs (W2 + W5)** — unblocked; makes it usable in a real
   Linux/WSL project this week. Analytics record automatically.
2. **Deploy to the first real non-alchamancer project**, let the journal accrue.
3. **Aggregator (W4)** once ≥2 projects have data.
4. **Windows port (W1 → W2-Windows)** — the largest track; parallelizable but gated on the
   hooks-on-Windows question.
5. **Baseline tag (W3)** at the first multi-project deployment.
6. **Hardening (W6)** as real usage surfaces needs.

## Open questions
- **Windows hooks:** how does Claude Code run `settings.json` hooks on native Windows?
  (Determines W1 hook effort.)
- **Analytics privacy:** journals hold verbatim payloads. For your own projects that's
  fine; if any project is shared, the aggregator should have a metrics-only (no-payload)
  mode.
- **Registry vs scan:** does the aggregator discover stores by scanning `~/Code/**/.legend`
  or read an explicit project list? (Explicit list is safer across machines.)
