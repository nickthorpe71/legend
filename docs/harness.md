# Test harness

`./check.sh` is the single gate. It builds everything with `-Werror` and runs the
full suite; it must be green before any commit.

## What check.sh runs

1. **Build** — four binaries: `legend` and `legend_test`, each also under
   ASan/UBSan (`-fsanitize=address,undefined,float-cast-overflow`). No
   `-ffast-math` — the spec needs strict IEEE.
2. **Unit tests** — `legend_test` (and its ASan build), embeddings off.
3. **Error paths** — no-store → `no_store`, idempotent `init`, oversized payloads.
4. **Fixtures** — `tests/fixtures/f01…f10.json` replayed through
   `harness/run.py`, each on the plain and ASan binary.
5. **Corpus replay** — the smoke and adversarial slices (below), replayed twice to
   assert byte-identical frame streams and snapshots, then scored against a
   **pinned baseline**.
6. **Fuzz (M5)** — `fuzz/fuzz_payload.py` (payload mutation) and
   `fuzz/fuzz_snapshot.py` (corrupt-snapshot reader), seeded for reproducibility.

## The replay corpus

`harness/corpus/` holds realistic `save`/`recall` traffic tracing one project's
life ("Alchamancer 2"):

- `episodes/e01…e11.json` — per-session gold payloads (`steps` of `verb` +
  `payload` + `now`). e01–e09 are the smoke slice; e10–e11 add adversarial cases.
- `probes_smoke.json`, `probes_adversarial.json` — recall probes grouped by kind
  (`current_state`, `cold_caller`, `deep_history`, `absent`, …), each with an
  expected answer.

## The tools

- `harness/gen_corpus.py --slice {smoke,adversarial}` — compiles a slice's episodes
  into a replay JSONL, **strictly validating** the probe file's schema (rejects
  unknown fields, requires `after_line`, etc.) exactly as the binary would.
- `harness/run.py --replay <jsonl>` / `--fixture <json>` — drives payloads and
  observe-probes through a `legend` binary.
- `harness/inspect.py --probes <file>` — scores probe outcomes and compares the
  metrics to the pinned baseline recorded in `harness/corpus/README.md`.

## Gotcha: probes_smoke.json is dual-consumer

`probes_smoke.json` is read both by `gen_corpus.py` (the strict, pinned gate above)
and by out-of-tree eval tooling that reads it leniently. **Don't add
eval-only fields to it**, and always run `./check.sh` after touching anything under
`harness/corpus/` — a schema-invalid probe fails the smoke gate with cascading
errors.
