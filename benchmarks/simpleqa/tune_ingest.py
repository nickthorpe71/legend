#!/usr/bin/env python3
"""Cheap single-page ingestion tuner.

Ingest ONE page into a throwaway store and print quality metrics, so ingestion
can be tuned for ~$1-2 per iteration instead of a full smoke. Uses the same
ingest_page() the real run uses, so anything that looks good here transfers.

    python tune_ingest.py corpus/pages/<sha>.md [--src LABEL] [--expect "2005"]

--expect is an optional gold string; the tuner recalls the page's main entity
and reports whether the expected value shows up in the frame (a proxy for
"the answerable fact is present and findable").
"""

import argparse
import json
import statistics
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import oai  # noqa: E402
from common.legend_io import Legend  # noqa: E402
from common.util import load_config, d, Journal  # noqa: E402
from ingest import ingest_page, new_counters, object_reject_reason  # noqa: E402

TUNE_STORE = "runs/tune/.legend"
TUNE_JOURNAL = "runs/tune/journal.jsonl"


def cost(usage, price):
    return usage["prompt_tokens"] / 1e6 * price["in"] + usage["completion_tokens"] / 1e6 * price["out"]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("page", help="path to a corpus page .md (relative to benchmarks/simpleqa or absolute)")
    ap.add_argument("--src", default=None, help="src label (default: page filename stem)")
    ap.add_argument("--expect", default=None, help="gold value that should be findable after ingest")
    args = ap.parse_args()

    cfg = load_config()
    if not cfg["models"].get("models_verified"):
        print("models_verified is false — run preflight.py --write first.")
        sys.exit(2)

    page = Path(args.page)
    rel = page if not page.is_absolute() else page.relative_to(d())
    if not (d() / rel).exists():
        print(f"no such page: {d() / rel}")
        sys.exit(1)
    src_label = args.src or Path(rel).stem

    # fresh throwaway store
    store = d(TUNE_STORE)
    if store.parent.exists():
        for f in store.parent.glob("**/*"):
            if f.is_file():
                f.unlink()
    lg = Legend(cfg["legend_binary"], store, now=cfg.get("legend_now"), embed=cfg.get("legend_embed", 1))
    lg.init(reset=True)

    counters = new_counters()
    model = cfg["models"]["ingester"]
    jpath = d(TUNE_JOURNAL)
    jpath.write_text("")
    with Journal(jpath) as journal:
        ingest_page(lg, str(rel), src_label, cfg, journal, counters, model)

    dump = lg.dump()
    n_elements = len(dump.get("elements", [])) - 32  # minus seed ontology
    el_names = [e.get("name", "") for e in dump.get("elements", [])[32:]]
    long_names = [n for n in el_names if len(n) > 45]

    price = cfg["pricing_per_mtok"]["ingester"]
    c = cost(counters["usage"], price)

    print("\n================ TUNE RESULT ================")
    print(f"page: {rel}  src={src_label!r}  model={model}")
    print(f"chunks cost: prompt={counters['usage']['prompt_tokens']:,} "
          f"completion={counters['usage']['completion_tokens']:,}  ${c:.2f}")
    print(f"tool calls: {counters['saves']} saves, {counters['recalls']} recalls")
    print(f"facts: kept={counters['kept_facts']}  rejected(non-atomic)={counters['rejected_facts']}")
    print(f"elements added: {n_elements}  (element names >45 chars: {len(long_names)})")
    if el_names:
        lens = [len(n) for n in el_names]
        print(f"element name length: mean={statistics.mean(lens):.0f} max={max(lens)}")

    # verdict heuristics
    verdict = []
    if n_elements > 120:
        verdict.append(f"BLOAT: {n_elements} elements for one page")
    if long_names:
        verdict.append(f"{len(long_names)} blob element names")
    if counters["rejected_facts"] > counters["kept_facts"]:
        verdict.append("more rejects than keeps — prompt still over-extracts")

    # findability of the expected answer — separate extraction from retrieval
    if args.expect:
        exp = args.expect.casefold()
        in_store = exp in json.dumps(dump, ensure_ascii=False).casefold()
        main_entity = src_label.replace("_", " ")
        frame = lg.recall({"focus": [main_entity], "limit": 20, "history_depth": 2})
        resolved = [f.get("name") for f in frame.get("focus", [])]
        in_frame = exp in json.dumps(frame, ensure_ascii=False).casefold()
        print(f"\nfindability of {args.expect!r}:")
        print(f"  in store at all (extraction): {in_store}")
        print(f"  in recall({main_entity!r}) frame (retrieval): {in_frame}  resolved={resolved}")
        if not in_store:
            verdict.append(f"EXTRACTION MISS: {args.expect!r} not in store")
        elif not in_frame:
            verdict.append(f"RETRIEVAL MISS: {args.expect!r} in store but not surfaced by entity recall")

    print("\nverdict:", "; ".join(verdict) if verdict else "CLEAN ✓")
    print("=============================================")
    print("\nInspect the store:")
    print(f"  LEGEND_STATE_DIR={store} {cfg['legend_binary']} dump --pretty | less")


if __name__ == "__main__":
    main()
