# Intent detection (Step 1)

Maps a text input to four intent scores in `[0, 1]`:
**conviction**, **prediction_error**, **arousal**, **curiosity**.
Each dimension has its own binary logistic-regression classifier baked
into the binary as a `.bin` blob.

## Pipeline

```
text
 ├── embed_text() ──────────────► [f32; 384]   (all-MiniLM-L6-v2 INT8, in-house BERT)
 └── extract_lexical_features() ► [f32; 34]    (hand-crafted: modals, person, mood, ...)
                                       │
                                       ▼
                                concat → [f32; 418]
                                       │
                                       ▼
                       sigmoid(w·x + b) per dim → [conv, pe, aro, cur]
```

The lexical features sit in front of the embedding as a
_front-door mediator_ (Pearl): linguistic surface form is causally
upstream of the embedding, so it captures intent signal cleanly while
the embedding carries intent confounded with topic.

## Files

| Path                                   | What                                                                      |
| -------------------------------------- | ------------------------------------------------------------------------- |
| `seed_pack.yaml` → `intent_prototypes` | High/low pole phrases + counterfactual pairs per dim                      |
| `src/embed.rs`                         | In-house INT8 BERT wrapper. `embed_text(&str) -> Vec<f32>`, `EMBEDDING_DIM = 384` |
| `src/lexical_features.rs`              | `extract_lexical_features(&str) -> [f32; 34]`                             |
| `src/intent_classifiers.rs`            | Loads the four `.bin` blobs at startup                                    |
| `src/intent_classifiers/<dim>.bin`     | Trained weights — 418 f32 + 1 f32 bias, little-endian                     |
| `src/steps/detect_intent.rs`           | Runtime: featurize + score                                                |
| `examples/gen_intent_classifiers.rs`   | **Trainer** — produces the `.bin` files                                   |
| `examples/audit_classifiers.rs`        | Diagnostics: weight cosines, top activators, paraphrase Δ                 |
| `examples/test_intent.rs`              | Held-out accuracy check (40 inputs across 8 groups)                       |
| `examples/shared/mod.rs`               | Serde types for the seed pack (dev-only deps)                             |

## Training objective

For each dim, full-batch gradient descent over two losses:

1. **Logistic regression** with class-weighted gradient + L2.
   Positives = own dim's `high_pole` + each `pairs[].high`.
   Negatives = own dim's `low_pole` + each `pairs[].low` + **every phrase
   from the other three dims** (cross-class negatives — Pearl Level-2:
   adjusting for "first-person assertion shape" as a confounder).
2. **Bradley-Terry contrastive** over own-dim `pairs` only.
   Per pair, minimize `-log sigmoid(w·(h - l))` so each high outscores
   its paired low. Pearl Level-3: same topic, flipped intent → forces
   the learned direction to be intent-axis-aligned, not topic-aligned.

Combined gradient per epoch:

```
w[j] -= lr * (g_log[j] + pair_weight * g_pair[j] + lambda * w[j])
b    -= lr *  g_log_b
```

Hyperparameters live in `gen_intent_classifiers.rs::main`:
`pos_weight = n_neg/n_pos` (auto), `pair_weight = 1.0`,
`lambda = 0.01`, `lr = 0.5`, `epochs = 5000`.

## Retrain

```bash
cargo run --release --example gen_intent_classifiers
```

Reads `seed_pack.yaml`, writes `src/intent_classifiers/*.bin`. The runtime
picks up the new weights at the next `cargo build` because the `.bin`
files are pulled in via `include_bytes!`.

Per-dim train output looks like:

```
wrote src/intent_classifiers/conviction.bin
  (pos=42, neg=293, pairs=14, pos_weight=6.98, log_loss=0.367, pair_loss=0.210)
```

Sanity: `pair_loss` should be well under `0.69` (= `-ln 0.5`). If it's
near or above that, the contrastive constraint isn't being satisfied —
either the pairs are inconsistent or `pair_weight` is too low.

## Validate

```bash
cargo run --release --example test_intent       # held-out accuracy
cargo run --release --example audit_classifiers # diagnostics
```

Current baseline (commit at the time of writing — see `git log`):

- **Test accuracy:** 39/40 (97.5%). The one failure is "I know for
  certain the bug is in the parser" — topic word "bug" pulls toward PE.
- **Inter-classifier weight cosines:** max |0.155|. Off-diagonal close
  to zero = classifiers are picking up orthogonal signals.
- **Top-8 activators per classifier:** 8/8 from own high pole on every
  dim. Zero hijacking from sibling dims.
- **Paraphrase invariance (max-Δ across 3 paraphrases):**
  conviction-high 0.18, conviction-low 0.11, prediction_error-high 0.12,
  arousal-high 0.21, curiosity-high 0.06.

Rough thresholds: `max-Δ < 0.10` is robust; `> 0.20` means the
classifier is keying on surface form rather than intent.

## Updating seed phrases

Edit `seed_pack.yaml` under `intent_prototypes.<dim>`. Each dim has:

- `high_pole: [String]` — phrases exemplifying the high pole
- `low_pole: [String]` — phrases exemplifying the low pole
- `pairs: [{ high: String, low: String }]` — counterfactual pairs.
  **Same topic, flipped intent.** These drive the contrastive loss, so
  inconsistent pairs (different topics, or where `high` doesn't actually
  outrank `low` on this dim) hurt training.

After editing, retrain (above) and re-run the validators.

### When to add a pair vs. a pole phrase

- Add to `high_pole`/`low_pole` when you want a new exemplar of the
  intent itself, regardless of topic.
- Add to `pairs` when you've found a topic the classifier confuses
  with another dim. The pair pins the topic and isolates the intent
  axis. Example: PE phrases mention "bug" a lot, so high-conviction
  sentences about bugs misclassify — fix with `(high: "I know for
certain the bug is in X", low: "I think maybe the bug is in X")`.

## Adding a new lexical feature

1. Bump `LEXICAL_FEATURE_COUNT` in `src/lexical_features.rs`.
2. Add the extraction in `extract_lexical_features` at the next index.
3. Retrain — the feature dimension changes, so old `.bin` files are
   invalid (parser will panic at startup until regenerated).

## Adding a new intent dimension

1. Add the dim's `PhrasePair` to `seed_pack.yaml` under
   `intent_prototypes`.
2. Add the field to `IntentPhrases` in `examples/shared/mod.rs` and
   `IntentClassifiers` in `src/intent_classifiers.rs`.
3. Extend `dims()` in `examples/shared/mod.rs`.
4. Add the `include_bytes!` line in `src/intent_classifiers.rs`.
5. Wire it through `src/steps/detect_intent.rs`.
6. Retrain.
