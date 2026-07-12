#!/usr/bin/env python3
"""Estimate full-run ingestion cost from the real page-size distribution.

Samples N questions (deterministic, same as fetch_corpus), fetches+extracts
their Wikipedia pages (no LLM — free), and projects Sol ingestion cost from the
measured chunk count and the per-chunk cost observed on the Haile page.

    python cost_survey.py 50
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import random  # noqa: E402
from common.util import load_config, est_tokens  # noqa: E402
import fetch_corpus as fc  # noqa: E402

# Per-chunk cost observed on the Haile page (exhaustive Sol): $2.27 / 5 chunks.
# Haile is fact-dense (worst case) so this anchors the HIGH end; a typical page
# produces fewer facts/chunk, so we also show a LOW end at ~55% of that.
SOL_CHUNK_HIGH = 2.27 / 5
SOL_CHUNK_LOW = SOL_CHUNK_HIGH * 0.55
TERRA_CHUNK = 0.37 / 5  # Terra on the same page, for reference


def main():
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 50
    cfg = load_config()
    cfg["n_questions"] = n
    rows = fc.load_rows(fc.download_dataset(cfg))
    picked = fc.sample(rows, cfg)

    seen, page_tokens, fails = {}, [], 0
    for i, q in enumerate(picked):
        for url in q["urls"]:
            if url in seen:
                continue
            seen[url] = True
            try:
                text = fc.fetch_and_extract(url, cfg)
                if text and len(text.strip()) >= 50:
                    page_tokens.append(est_tokens(text))
                else:
                    fails += 1
            except Exception:
                fails += 1
        print(f"\r  fetched {i + 1}/{len(picked)} questions...", end="", flush=True)
    print()

    chunk_tok = cfg["ingest_chunk_tokens"]
    total_tokens = sum(page_tokens)
    total_chunks = sum(max(1, round(t / chunk_tok)) for t in page_tokens)
    pages = len(page_tokens)

    print(f"\n=== page survey (n={n} questions) ===")
    print(f"unique pages fetched ok: {pages}  (fetch failures: {fails})")
    print(f"pages per question: {pages / len(picked):.2f}")
    print(f"page tokens: total={total_tokens:,}  mean={total_tokens // max(1,pages):,}  "
          f"max={max(page_tokens) if page_tokens else 0:,}")
    print(f"chunks (@{chunk_tok} tok): {total_chunks}")

    print(f"\n=== projected INGESTION cost ===")
    print(f"Sol:   ${total_chunks * SOL_CHUNK_LOW:6.2f} (low)  ..  ${total_chunks * SOL_CHUNK_HIGH:6.2f} (high, Haile-dense)")
    print(f"Terra: ${total_chunks * TERRA_CHUNK:6.2f} (ref; but Terra misses summary facts)")
    print("\n(arms + grading add ~$2-4 for n=50; scale ~linearly with n)")


if __name__ == "__main__":
    main()
