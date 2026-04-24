# Chunking + extraction evaluation (#19)

**Recorded:** 2026-04-24
**Tool:** `examples/chunking_eval.rs`
**Dataset:** 10 synthetic samples spanning the project_alpha signal/noise
fixture, structured tick prefixes, paragraph breaks, pipe-separated lists,
long single sentences, and numeric-heavy text.

Raw report (committed): `.perf/chunking-eval-2026-04-24.md`.

## Headline finding

**The chunking layer is not the bottleneck.** The entity and relation
extraction is where the noise comes from.

The median chunk size (111 chars) and chunk boundaries look sensible.
What drops an over-stuffed graph on the observability fixtures is the
per-chunk extraction, which over-generates:

- **17 entities per chunk** on average
- **48 relations per chunk** on average
- Noun-heads get split into fragments (`SQLite` → `sqlite`, `Lite`; `Project
  Alpha` → `Project`, `alpha`) and re-emitted as separate entities
- Verb tokens (`moved`, `back`) get extracted as `Term` entities and
  re-appear in positional relations
- Compound phrases over-generate (`Alpha audit`, `Project Alpha audit`,
  and `audit log` all appear alongside `Alpha audit log`)
- The O(n²) relation pass pairs every extracted token with every other,
  producing spurious triples like `(purple, located_near, stapler)` when
  "purple" and "stapler" are part of the same noun phrase

## Implications for the queue

1. **#20 (ML-based chunk boundary detection) will not fix the
   observability failures on its own.** Median chunk size is already
   short (~111 chars on this dataset); re-splitting would not reduce
   the duplicate graph nodes or the spurious relations.
2. **#21 (batch vs whole-text embedding)** is an orthogonal performance
   choice. Extraction happens on the chunk after chunking; changing the
   embedding unit doesn't affect the entity fragmentation.
3. **#17 (the observability baseline) is primarily an extraction
   problem, not a chunking problem.** It needs:
   - Noun-phrase canonicalization before entities enter the graph
   - A merging step that collapses morphological fragments
     (`sqlite`/`Lite`/`SQLite` → one node)
   - Relation extraction that treats a phrase as one argument, not as
     all its sub-tokens

## Per-sample observations

| Sample | Input | Chunks | Entities | Relations | Notes |
|--------|-------|--------|----------|-----------|-------|
| alpha_001_signal_noise_mix | 121 chars | 1 | 22 | 48 | `SQLite`/`sqlite`/`Lite` triples; `purple stapler` split into `purple`+`stapler` |
| alpha_002_repeated_and_incidental | 108 chars | 1 | 24 | 33 | `Project Alpha audit` / `Alpha audit log` overlap |
| alpha_004_backup_time | ~120 chars | 1 | ~20 | ~60 | `02:30 UTC` captured as Value ✓ |
| alpha_005_migration_path | ~130 chars | 1 | ~20 | ~60 | `db/migrations` captured as FilePath ✓ |
| paragraph_break | 218 chars | 2 | — | — | `\n\n` split worked correctly |
| pipe_separated | 48 chars | 4 | — | — | `|` split into 4 chunks, one task each ✓ |
| long_single_sentence | 407 chars | 1 | — | — | Single sentence, not re-split into smaller units |
| numeric_heavy | 91 chars | 1 | — | — | Captures `20ms`, `94ms`, `42ms`, `10`, `11k` as Values |

(Full per-chunk entity and relation listings in the raw report.)

## Chunking behavior

Current `chunk_text` pipeline (`src/memory/entorhinal.rs:577`):

1. Hard split on `\n\n` paragraph breaks and `|` pipe separators.
2. Hard split on topic-shift markers (case-insensitive).
3. Group adjacent sentences up to `EPISODIC_CHUNK_TARGET_CHARS` per chunk.

**What works:** `\n\n` and `|` boundaries, topic-shift markers,
size-bounded grouping.

**What doesn't:** No protection against a single sentence growing past
the target size (long_single_sentence at 407 chars stays as one chunk).
That's fine in isolation; combined with extraction's O(n²) relation
blowup it amplifies cost.

## Recommended next steps (out of scope for #19)

1. **Entity canonicalization**: after `extract_entities`, collapse
   fragments that appear as sub-strings of a longer extracted phrase to
   the longer form. (`SQLite`, `Lite`, `sqlite` → `SQLite`.)
2. **Filter verb tokens** from `Term` kind entity lists before relation
   extraction. Only noun phrases should participate in relations.
3. **Relation extraction constraint**: only emit one relation per
   (head, relation, tail) noun-phrase pair, not per all sub-token
   permutations.
4. **Then re-run project_alpha (#17)**: expect the `SQLite occurred N
   times` assertion to pass, and most of the `(entity, kind, entity)`
   edge assertions to pass with the right kinds.

## How to re-run

```bash
cargo run --release --example chunking_eval > .perf/chunking-eval-$(date +%F).md
```

The sample list lives in `examples/chunking_eval.rs::SAMPLES`. Add
fixtures there to extend the corpus as the extraction changes.
