#!/usr/bin/env python3
"""Subset re-ingest with a TIGHTENED extractor, to test whether reducing
over-extraction (205 elem/page) improves retrieval. Ingests only the pages behind
the 13 D-only miss questions into a fresh store (store_tight/), with the ingester
prompt's exhaustiveness clause replaced by a precision-first one. Compare against
the over-extracted store with measure_tight.py.
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import ingest  # noqa: E402
from common import prompts  # noqa: E402
from common.legend_io import from_config  # noqa: E402
from common.util import load_config, d, Journal  # noqa: E402

MISS_PAGES = {
    "corpus/pages/08d3ce3d.md", "corpus/pages/161804c0.md", "corpus/pages/1dc01276.md",
    "corpus/pages/3c583a61.md", "corpus/pages/40bf1d25.md", "corpus/pages/4e3d05ce.md",
    "corpus/pages/57e40aae.md", "corpus/pages/5b230ac2.md", "corpus/pages/99d31bd1.md",
    "corpus/pages/a0904ecf.md", "corpus/pages/a537d953.md", "corpus/pages/a865f912.md",
    "corpus/pages/d36b1897.md", "corpus/pages/d56b8768.md", "corpus/pages/fc3d381b.md",
}

# Tighten: replace the "be thorough, a dense page may yield many facts" clause.
NEW = ("- THEN record only the specific facts a fact-seeking question would plausibly\n"
       "  ask about — the salient dates, names, numbers, roles, and results tied to the\n"
       "  MAIN entities of the page. STOP THERE. Do NOT exhaustively extract: skip\n"
       "  technical/physical parameters, orbital or numeric constants, exhaustive\n"
       "  lists/enumerations, minor cross-references, and anything recorded only for\n"
       "  completeness. A dense page does NOT license many facts — choose the few that\n"
       "  matter (aim well under ~15 facts per chunk, not many dozens). Never create a\n"
       "  fact or element whose subject is a citation, source pointer, or reference.")
TIGHT = re.sub(r"- THEN be thorough with the specifics:.*?real entity\.", NEW,
               prompts.INGESTER_SYSTEM, flags=re.DOTALL)
assert TIGHT != prompts.INGESTER_SYSTEM, "prompt replacement did not fire"
ingest.INGESTER_SYSTEM = TIGHT  # ingest_page reads this module global


def main():
    cfg = load_config()
    lg = from_config(cfg, d("store_tight", ".legend"))
    lg.init(reset=True)
    pages = [(p, s) for p, s in ingest.pages_to_ingest(cfg) if p in MISS_PAGES]
    print(f"tight re-ingest: {len(pages)} pages")
    counters = ingest.new_counters()
    with Journal(d("runs", "ingest_tight", "journal.jsonl")) as j:
        for path, src in pages:
            before = counters["saves"]
            ingest.ingest_page(lg, path, src, cfg, j, counters, cfg["models"]["ingester"])
            print(f"  {src}: +{counters['saves'] - before} saves")
    dump = lg.dump()
    ne = len([e for e in dump.get("elements", []) if e.get("redirect") is None])
    u = counters["usage"]
    cost = u["prompt_tokens"] / 1e6 * 5 + u["completion_tokens"] / 1e6 * 30
    print(f"\nTIGHT store: {len(pages)} pages, {ne} elements, {counters['kept_facts']} facts, "
          f"{counters['rejected_facts']} rejected")
    print(f"  elem/page = {ne / len(pages):.0f}  (over-extracted store was ~205)")
    print(f"  ingest cost = ${cost:.2f}")


if __name__ == "__main__":
    main()
