"""
Phase 0 exploration. Goals:

1. Pick the smallest GLiNER variant that satisfies v0's needs:
   - zero-shot NER (mandatory)
   - zero-shot relation extraction (preferred; falls back to a
     separate model if not available in one package)
2. Inspect the model architecture so we know what tensors to extract.
3. Run a sanity inference and print labeled spans.

Run:
    oracle/venv/bin/python oracle/01_explore_gliner.py

Caches the model under oracle/hf_cache so we don't re-download.
"""

import json
import os
import sys
from pathlib import Path

os.environ["HF_HOME"] = str(Path(__file__).parent / "hf_cache")
os.environ["TRANSFORMERS_VERBOSITY"] = "error"

from gliner import GLiNER  # noqa: E402

# `urchade/gliner_small-v2.1` is the smallest of the v2.1 line — DeBERTa-v3-small
# backbone (33M backbone + GLiNER head, total ~165M params). NER-only, but
# the encoder is identical to the multitask variants. We'll start here, then
# swap to a multitask variant for RE in Phase 5 if needed.
MODEL_ID = "urchade/gliner_small-v2.1"

print(f"loading {MODEL_ID} …", flush=True)
model = GLiNER.from_pretrained(MODEL_ID)
model.eval()
print("loaded", flush=True)

# Architecture inspection: report what the model carries so we can plan
# weight extraction.
print()
print("=" * 60)
print("ARCHITECTURE")
print("=" * 60)

total_params = sum(p.numel() for p in model.parameters())
print(f"total params: {total_params:,}")

# The "backbone" is the DeBERTa encoder; the rest is the GLiNER head.
backbone = model.model.token_rep_layer if hasattr(model.model, "token_rep_layer") else None
if backbone is None:
    # Newer gliner versions
    for name, mod in model.named_modules():
        if "backbone" in name.lower() or "deberta" in name.lower():
            print(f"  candidate backbone: {name}: {type(mod).__name__}")

print()
print("top-level modules:")
for name, mod in model.named_children():
    n_params = sum(p.numel() for p in mod.parameters())
    print(f"  {name}: {type(mod).__name__}  ({n_params:,} params)")

print()
print("second-level under .model:")
if hasattr(model, "model"):
    for name, mod in model.model.named_children():
        n_params = sum(p.numel() for p in mod.parameters())
        print(f"  model.{name}: {type(mod).__name__}  ({n_params:,} params)")

# Save the parameter name → shape map. Hard requirement for Phase 1's
# weight-extractor: we need to know every tensor name to look up.
param_map = {name: list(p.shape) for name, p in model.named_parameters()}
param_path = Path(__file__).parent / "params.json"
with open(param_path, "w") as f:
    json.dump(param_map, f, indent=2)
print(f"\nwrote {param_path} ({len(param_map)} tensors)")

# Quick inference test — sanity check that the model works end-to-end.
print()
print("=" * 60)
print("INFERENCE SANITY")
print("=" * 60)
text = "My dentist appointment with Dr. Rao changed from Tuesday to Friday."
labels = ["person", "event", "weekday", "role"]
entities = model.predict_entities(text, labels, threshold=0.3)
print(f"input: {text}")
print(f"labels: {labels}")
for e in entities:
    print(f"  [{e['start']}:{e['end']}]  {e['label']:<10}  '{e['text']}'  ({e['score']:.3f})")
