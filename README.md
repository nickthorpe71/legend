# Legend

Long-term memory for LLMs. v2 rewrite in Rust.

## Status

v0 in progress. Step 1 (per-dimension intent detection) is built and
tested; Steps 2–13 are next. See `new_foundation_v0_core.md` for the v0
scope.

## Build

```bash
cargo build --release
```

The all-MiniLM-L6-v2 ONNX model (~23 MB) and the four trained intent
classifiers are baked into the binary via `include_bytes!` — no
network access or external files at runtime.

## Try it

```bash
./target/release/legend "I am absolutely certain that the meeting is at 3pm"
# conviction       0.51
# prediction_error 0.33
# arousal          0.41
# curiosity        0.25
```

## Tests / benchmarks

```bash
cargo run --release --example test_intent       # held-out accuracy
cargo run --release --example audit_classifiers # diagnostics
cargo bench                                     # criterion benches
```

## Docs

- `new_foundation.md` / `new_foundation_v0_core.md` — design (source of truth)
- `R-STAR.md` — Rust style guide for this repo
- `docs/` — operate notes (retraining, validation, per-step internals)
