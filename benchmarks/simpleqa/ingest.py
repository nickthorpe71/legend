#!/usr/bin/env python3
"""Step 3: ingest the corpus into one Legend store with the strong model (Sol).

Pages (deduped, in manifest order) are split into ~2k-token chunks; each chunk
runs a save/recall tool-loop so the model reuses canonical names instead of
minting duplicates. Every request/tool-call/frame is journaled, and a per-page
high-water mark makes a re-run resume rather than restart (each save is durable
on disk, so re-entry at page N is safe).

Failure policy: transient API errors retry with backoff inside oai (never
skipped). A chunk whose loop still raises keeps whatever partial saves landed,
is logged, and the run continues — a bad page must not sink the store.

    python ingest.py            # real ingestion (needs OPENAI_API_KEY)
    python ingest.py --dry-run  # plumbing only: one deterministic save per chunk
    python ingest.py --fresh    # force re-init even if a store exists
"""

import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import oai  # noqa: E402
from common.legend_io import from_config  # noqa: E402
from common.prompts import INGESTER_SYSTEM  # noqa: E402
from common.schemas import SAVE_TOOLS  # noqa: E402
from common.util import load_config, d, chunk_text, Journal, read_jsonl  # noqa: E402

STORE_DIR = "store/.legend"
PROGRESS = "runs/ingest/progress.json"
JOURNAL = "runs/ingest/journal.jsonl"
SUMMARY = "runs/ingest/summary.json"


def pages_to_ingest(cfg):
    """Unique fetched pages for surviving questions, in first-seen manifest order.
    Returns list of (path, src_label)."""
    survivors = {q["qid"] for q in read_jsonl(d("corpus", "questions_supported.jsonl"))}
    seen, out = set(), []
    for m in read_jsonl(d("corpus", "manifest.jsonl")):
        if not m.get("ok") or m["qid"] not in survivors:
            continue
        if m["path"] in seen:
            continue
        seen.add(m["path"])
        out.append((m["path"], m["url"]))
    return out


def trim_frame(frame):
    """Bounded view of a recall frame for the model — enough to check existence
    and look facts up, without flooding the context."""
    keep = ("resolution", "near_matches", "focus", "state", "constraints",
            "decisions", "recent", "history", "related", "sources")
    return {k: frame[k] for k in keep if k in frame and frame[k]}


def make_dispatch(lg, src_label, journal, counters):
    def dispatch(name, args):
        if name == "legend_recall":
            frame = lg.recall({
                "focus": args.get("focus", []),
                "limit": args.get("limit", 20),
                "history_depth": args.get("history_depth", 3),
            })
            journal.write({"event": "recall", "src": src_label, "args": args, "frame": trim_frame(frame)})
            return json.dumps(trim_frame(frame), ensure_ascii=False)
        if name == "legend_save":
            # ensure every fact carries src, default to this page
            facts = []
            for f in args.get("facts", []):
                f = dict(f)
                f.setdefault("src", src_label)
                facts.append(f)
            payload = {
                "source": args.get("source", src_label),
                "elements": args.get("elements", []),
                "facts": facts,
                "changes": args.get("changes", []),
                "retract": args.get("retract", []),
                "merge": args.get("merge", []),
            }
            res = lg.save(payload)
            counters["saves"] += 1
            counters["minted_elements"] += len(res.get("writes", {}).get("minted_elements", []))
            counters["minted_relations"] += len(res.get("writes", {}).get("minted_relations", []))
            out = {
                "writes": res.get("writes", {}),
                "near_matches": res.get("near_matches", []),
                "conflicts": res.get("conflicts", []),
            }
            journal.write({"event": "save", "src": src_label, "payload": payload, "result": out})
            return json.dumps(out, ensure_ascii=False)
        return json.dumps({"error": f"unknown tool {name}"})
    return dispatch


def dry_dispatch_page(lg, path, src_label, journal, counters):
    """No-OpenAI plumbing test: one save per chunk from the first line of text."""
    text = (d() / path).read_text(encoding="utf-8")
    for i, chunk in enumerate(chunk_text(text, 2000)):
        title = next((ln.strip("# ").strip() for ln in chunk.splitlines() if ln.strip()), f"chunk{i}")
        payload = {
            "source": src_label,
            "elements": [{"name": title[:60], "kind": "concept", "summary": "dry-run element"}],
            "facts": [{"s": title[:60], "p": "mentioned_in", "o": src_label, "src": src_label}],
        }
        res = lg.save(payload)
        counters["saves"] += 1
        counters["minted_elements"] += len(res.get("writes", {}).get("minted_elements", []))
        journal.write({"event": "dry_save", "src": src_label, "chunk": i, "result": res.get("writes", {})})


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--fresh", action="store_true")
    args = ap.parse_args()

    cfg = load_config()
    if not args.dry_run and not cfg["models"].get("models_verified"):
        print("Refusing to spend tokens: models_verified is false. Run preflight.py --write first.")
        sys.exit(2)

    lg = from_config(cfg, d(STORE_DIR))
    progress_path = d(PROGRESS)
    done = set()
    if progress_path.exists() and not args.fresh:
        done = set(json.loads(progress_path.read_text()).get("done", []))

    if done and not args.fresh:
        print(f"resuming — {len(done)} page(s) already done")
    else:
        lg.init(reset=True)
        done = set()
        progress_path.parent.mkdir(parents=True, exist_ok=True)
        progress_path.write_text(json.dumps({"done": []}))

    pages = pages_to_ingest(cfg)
    counters = {"saves": 0, "minted_elements": 0, "minted_relations": 0}
    per_page_saves = {}
    model = cfg["models"]["ingester"]

    with Journal(d(JOURNAL)) as journal:
        for path, src_label in pages:
            if path in done:
                continue
            before = counters["saves"]
            journal.write({"event": "page_start", "path": path, "src": src_label})
            dispatch = make_dispatch(lg, src_label, journal, counters)

            if args.dry_run:
                dry_dispatch_page(lg, path, src_label, journal, counters)
            else:
                text = (d() / path).read_text(encoding="utf-8")
                chunks = chunk_text(text, cfg["ingest_chunk_tokens"])
                for i, chunk in enumerate(chunks):
                    user = (
                        f"Reference page: {src_label}\n"
                        f"Chunk {i + 1}/{len(chunks)}. Record the durable facts this chunk states "
                        f"(use src={src_label!r} on facts). Recall before you save.\n\n{chunk}"
                    )
                    try:
                        res = oai.run_tool_loop(
                            model=model, system=INGESTER_SYSTEM, user=user,
                            tools=SAVE_TOOLS, dispatch=dispatch,
                            max_calls=cfg["ingest_max_calls_per_chunk"], seed=cfg.get("seed"),
                        )
                        journal.write({"event": "chunk_done", "src": src_label, "chunk": i,
                                      "stop_reason": res["stop_reason"], "calls": res["tool_calls_made"],
                                      "usage": res["usage_total"]})
                    except Exception as e:
                        journal.write({"event": "chunk_error", "src": src_label, "chunk": i,
                                      "error": f"{type(e).__name__}: {e}"})
                        print(f"  WARN chunk {i} of {src_label} errored: {e} — keeping partial saves")

            per_page_saves[path] = counters["saves"] - before
            done.add(path)
            progress_path.write_text(json.dumps({"done": sorted(done)}))
            print(f"  page done: {src_label} (+{per_page_saves[path]} saves)")

    dump = lg.dump()
    n_elements = len(dump.get("elements", []))
    summary = {
        "pages": len(pages),
        "elements_total": n_elements,
        "saves": counters["saves"],
        "minted_elements": counters["minted_elements"],
        "minted_relations": counters["minted_relations"],
        "per_page_saves": per_page_saves,
        "model": model,
        "dry_run": args.dry_run,
    }
    d(SUMMARY).write_text(json.dumps(summary, indent=2))
    print(json.dumps(summary, indent=2))

    # asserts
    zero_pages = [p for p, n in per_page_saves.items() if n == 0]
    if n_elements <= 32:
        print("FATAL: store has only the seed ontology — nothing was ingested.")
        sys.exit(1)
    if zero_pages:
        print(f"WARN: {len(zero_pages)} page(s) produced 0 saves: {zero_pages}")


if __name__ == "__main__":
    main()
