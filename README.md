# Legend

Long-term memory for LLMs. v2 rewrite in Rust.

## Status

v0 in progress. Built and tested:
- Step 1 — per-dimension intent detection
- Step 2 — policy adjustment
- Seeded substrate — 72 elements / 53 relations boot from `seed_pack.yaml`

Next: Step 4 region routing, then extractors. See
`new_foundation_v0_core.md` for the v0 scope.

## Build

```bash
cargo build --release
```

Baked into the binary via `include_bytes!` — no network access or
external files at runtime:
- all-MiniLM-L6-v2 ONNX model (~23 MB)
- Four trained intent classifiers
- Seed hypergraph (~120 KB)

## Try it

```bash
./target/release/legend "I am absolutely certain that the meeting is at 3pm"
# intent
#   conviction       0.507
#   prediction_error 0.331
#   arousal          0.413
#   curiosity        0.246
# policy (adjusted)
#   default_conf           0.420
#   ...
# seed graph
#   elements         72
#   relations        53
#   region children of GENESIS  15
```

## Persistence + git merge driver

The substrate is saved to `./.legend/memory.lz4` after every tick and
loaded at the top of the next run, so memory carries forward across
process restarts. To skip the load for a single run:

```bash
LEGEND_RESET=1 ./target/release/legend "..."
```

`.legend/memory.lz4` is committed alongside source. When two branches
both mutate it, git can't text-merge a binary file — register the
substrate-aware merge driver in a fresh clone:

```bash
./target/release/legend init
```

That writes `git config --local merge.legend.driver` and adds the
`.gitattributes` rule. After it, `git merge` reconciles `.legend/memory.lz4`
automatically: elements unify by `(name, polarity)`, relations dedup
after id-remap, and conflicting statuses resolve as Retracted >
Superseded > Asserted > Entailed > Defeasible.

## Tests / benchmarks

```bash
cargo test --lib                                # unit tests
cargo run --release --example test_intent       # held-out intent accuracy
cargo run --release --example audit_classifiers # classifier diagnostics
cargo run --release --example dump_hypergraph_md  # snapshot to inspect/seed.md
cargo bench                                     # criterion benches
```

## Regenerate baked artifacts

```bash
cargo run --release --example gen_intent_classifiers  # → src/intent_classifiers/*.bin
cargo run --release --example gen_seed_graph          # → src/seed/graph.bin
```

## Docs

- `new_foundation.md` / `new_foundation_v0_core.md` — design (source of truth)
- `R-STAR.md` — Rust style guide for this repo
- `docs/` — operate notes (retraining, validation, per-step internals)
