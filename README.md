# Legend

Long-term memory for LLMs. v2 rewrite in Rust.

## Status

v0 end-to-end. The 12-step tick pipeline (intent → policy → routing →
extractors → coref → build relations → supersede → hebbian → decay →
frame) runs against a 622-element / 610-relation seeded substrate.
Source-of-truth design: `new_foundation.md`, `new_foundation_v0_core.md`.

## Build

```bash
cargo build --release
```

Baked into the binary via `include_bytes!` — no network access or
external files at runtime:

- all-MiniLM-L6-v2 INT8 weights (~22 MB) + GLiNER1 INT8 weights
- Four trained intent classifiers
- Seed hypergraph (`src/seed/graph.bin`, ~1 MB)

## Try it

Two execution modes share the same tick code path:

```bash
# Daemon (default): auto-starts on first call, amortizes cold-start.
./target/release/legend "I am absolutely certain that the meeting is at 3pm"
# Prints the rendered ConsciousAttentionFrame.

./target/release/legend start    # launch daemon in the background
./target/release/legend status   # pid, uptime, substrate sizes
./target/release/legend stop     # graceful shutdown
```

```bash
# In-process: full Step 1–12 verbose dump, prints every intermediate.
LEGEND_INPROC=1 ./target/release/legend "..."
LEGEND_TIME=1   ./target/release/legend "..."   # per-step timings
LEGEND_RESET=1  ./target/release/legend "..."   # skip on-disk snapshot
```

The daemon listens on TCP loopback; clients discover the port via
`.legend/legend.port`. A `fs2` exclusive flock on `.legend/legend.lock`
guarantees single-writer.

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
cargo test --test v0_acceptance                 # end-to-end tick acceptance
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
- `docs/` — operate notes (seed graph, inference engine, intent detection, inspection)
