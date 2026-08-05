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

> **RE-AUDITED 2026-08-04. The "~6 shims" estimate below is wrong.** A full
> source sweep found **23 shim sites in `legend.c`+`embed.c`, five of them
> semantic differences rather than shims**, plus ~130 mechanical sites in
> `legend_test.c` (the gate cannot run on Windows without them) and
> `legend_viz.c`, which is X11 and should be **excluded from the Windows
> target** entirely. Four of the five line references below are also stale.
> Spot-verified by hand: the `rename`, binary-mode, path-parsing, and `st_mtim`
> findings are all confirmed against current source.
>
> **The five that are not shims — do these first, in this order:**
>
> 1. **`rename()` over an existing file fails on Windows** (`legend.c:8552`).
>    `legend.snapshot` always exists after `init`, so **every save after the
>    first fails**. `MoveFileExA(..., MOVEFILE_REPLACE_EXISTING)` is the
>    replacement, but it is still not equivalent: it fails with
>    `ERROR_SHARING_VIOLATION` if any process holds the destination open without
>    `FILE_SHARE_DELETE`, and MSVCRT's `open`/`fopen` never pass that flag.
>
>    **Scope corrected 2026-08-04** — an earlier note here called this a design
>    problem on the grounds that `mcp_serve` keeps the snapshot open outside the
>    lock. It does not. `snapshot_load` (`legend.c:8661-8690`) opens, `fstat`s,
>    reads the whole file into `g_snap_buf`, and **closes the fd before
>    parsing**; a warm server holds the parsed graph in memory, not a file
>    handle. The exposure is therefore a narrow race — a save landing in the
>    millisecond window while another process is reading the snapshot — not a
>    structural conflict, and `legend_viz.c:1148` follows the same
>    open-read-close shape. Fix it by opening the snapshot for reading via
>    `CreateFileA` with `FILE_SHARE_READ|FILE_SHARE_WRITE|FILE_SHARE_DELETE` +
>    `_open_osfhandle`, which restores exactly the POSIX semantics the code
>    already assumes. No architecture change, and the documented non-blocking
>    hook behavior stays as-is.
> 2. **Text-mode translation silently corrupts the snapshot.** The four snapshot
>    fds (`legend.c:8531/8536/8661/8681`) need `_O_BINARY`, and the journal
>    (`legend.c:8946`, `fopen(path,"a")`) needs `"ab"`. Without it every `0x0A`
>    becomes `0x0D 0x0A`, the declared length stops matching `st_size`, and a
>    store written on Windows is unreadable on Linux. Fails at the format level,
>    silently, with no compile error.
> 3. **`strrchr(path, '/')` over OS-returned paths.** `getcwd` returns
>    backslashes, so `discover_store` (`legend.c:9031`, root logic at `:9034`)
>    returns "no store" for every invocation without `LEGEND_STATE_DIR`, and
>    `embed.c:469` never finds the model dir so **embeddings silently disable**.
>    Needs a separator helper plus drive-root-aware walk-up, not a `#define`.
>    (The roadmap's "forward slashes work in fopen" is true and irrelevant — the
>    problem is parsing paths the OS hands back, not building them.)
> 4. **`st_mtim.tv_nsec` does not exist on Windows** (`legend.c:10775-10776`) —
>    hard compile error, and glibc-specific even on Unix. Use `FILETIME`, not
>    `st_mtime`: dropping to second resolution lets the warm-graph gate miss an
>    external write landing in the same second.
> 5. **`tmpfile()`** (`legend.c:9178` `--pretty`, `legend.c:10644` every MCP
>    `tools/call`). MSVCRT creates it in the drive root, which fails unelevated.
>    One path kills `--pretty` (which is what the generated hooks invoke); the
>    other falls through to writing frame JSON onto the MCP stdout channel,
>    **corrupting the JSON-RPC stream**.
>
> **Toolchain: MinGW-w64, not MSVC** — and more strongly than stated below. The
> blocker is GCC *extensions* in `embed.c` (`__attribute__((target("avx2,fma")))`
> at `embed.c:171`, `__builtin_cpu_supports` at `:192`) plus missing `getline`,
> `S_ISDIR`, `O_CLOEXEC`, and `ssize_t`. MinGW supplies all of them. Also note
> `embed.c:29` gates SIMD on `__x86_64__`, which MSVC does not define, so MSVC
> would silently compile out the SIMD path.
>
> **`legend.lock` case sensitivity** (`legend.c:8571`): the orphan-tmp sweep
> skips the lock by `strcmp`. NTFS is case-insensitive but case-preserving, so a
> `Legend.lock` is the same file yet fails the compare — and the sweep would
> **unlink the live lock**. Needs `_stricmp`.

The original estimate, kept for reference (line numbers stale as noted):

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

### W1b — Hooks and generated config (DONE 2026-08-05)

`legend init` used to write three **bash** hooks — pipes, `sed`, `tr`,
`head -c`, command substitution. Claude Code on native Windows runs hooks under
**PowerShell** unless Git for Windows is installed, so all three failed there,
and that is what a Windows user hits *before* anything about the store matters.

All three are now **exec form** (a command plus an argv, no shell on any
platform), because the logic moved into the binary:

| was | now |
|---|---|
| `\| head -c 20000` | `--max-bytes N`, capped in-process |
| the UserPromptSubmit `sed`/`tr`/debounce pipeline | `legend hook prompt` |
| the Stop `git diff \| wc -l` nudge | `legend hook stop` |

`legend hook prompt` reads the hook's JSON on stdin, applies the 20-second
debounce, skips system-injected blocks (they arrive as prompts and open with
`<`), sanitizes the text to a focus phrase, and then **rewrites itself into an
ordinary observe-recall** — the hook is a caller of recall, not a second
implementation of it, so it inherits the frame, the lock discipline and the
journal line.

`legend hook stop` replaces the changed-file count with a better-aimed question
the journal can answer without git or a shell: *this session recalled and saved
nothing*. Session start is the last focus-less recall.

Note this removes the last need for `SIGPIPE` handling in the hook path — there
is no pipe to be truncated any more.

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
