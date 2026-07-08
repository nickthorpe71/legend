# Embeddings

Legend has a pure-C embedder (`embed.c`) — no Python, no ONNX runtime, no daemon.
It powers the lower tiers of recall resolution: when a focus term isn't an exact
name, alias, or lexical hit, it's embedded and ranked against element vectors by
cosine.

## The model

- **BGE-small-en-v1.5** — a 12-layer BERT (384-dim, bert-base-uncased vocab),
  quantized to per-row int8. WordPiece tokenizer → transformer → **CLS pooling**
  (row 0) → L2 normalize. The dot product is AVX2/FMA where available.
- Runtime assets live in `models/bge-small-en-v1.5/`: the int8 weight blob and
  `vocab.txt`. That's all the loaded model needs.

## Resolution tiers

`recall` resolves each focus term through: exact name → alias → lexical → embedding.
The embedding tier uses **asymmetric retrieval** — the query carries a BGE search
instruction prefix, while element vectors are embedded bare — which lifts distant
paraphrases (e.g. "spell resource costs" → the `mana economy` element).

## The sidecar

Element vectors are cached in a `vectors.bin` sidecar, refreshed eagerly on each
`save` so recall never blocks on embedding a cold graph. The sidecar is keyed to
the model blob's fingerprint: swap the model and it auto-invalidates and re-embeds.

`LEGEND_EMBED=0` disables the embedder entirely (recall falls back to the exact/
alias/lexical tiers); `LEGEND_EMBED_DIR` points at a different model directory.

## Re-exporting the model blob

The runtime blob is produced offline by `tools/embed_prep/` (Rust + candle). The
130 MB fp32 safetensors source is **not** committed — fetch it, then export:

```bash
curl -sL -o models/bge-small-en-v1.5/model.safetensors \
  https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/main/model.safetensors
cd tools/embed_prep && cargo run --release -- \
  ../../models/bge-small-en-v1.5/model.safetensors
```

That writes the int8 blob, an id-ordered `vocab.txt`, and `golden.txt` (reference
vectors from candle). Validate the C encoder against it:

```bash
cc -O2 -std=c99 -DEMBED_TEST -o embed_test embed.c -lm && ./embed_test
# expects 8/8 samples with cosine > 0.999 vs candle
```

## Swapping to a different model

`embed.c` validates its blob header against compile-time constants
(`HID`, `NL`, `NH`, `INTER`, `NVOCAB`, `MAXPOS`). A same-family BERT that differs
only in depth or pooling needs just those constants updated plus a re-export; a
different hidden size also changes the sidecar vector width. Keep to standard BERT
(absolute positions, `MAXPOS` 512) — long-context variants change positional
encoding and won't load.
