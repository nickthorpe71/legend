#!/usr/bin/env python3
"""Step 3: ingest the corpus into one Legend store with the strong model (Sol).

Pages (deduped, in manifest order) are split into ~2k-token chunks; each chunk
runs a save/recall tool-loop so the model reuses canonical names instead of
minting duplicates. Every request/tool-call/frame is journaled, and a per-page
high-water mark makes a re-run resume rather than restart.

Ingestion quality is enforced here, not merely requested: the save dispatch
rejects facts whose object is not atomic (prose/lists/over-long), so a model
that over-extracts cannot silently bloat the store — it gets the rejects back
and can re-submit them decomposed. Recall frames returned during ingestion are
trimmed hard (resolved names + a few scored candidates) to keep the dedup loop
cheap.

    python ingest.py            # real ingestion (needs OPENAI_API_KEY)
    python ingest.py --dry-run  # plumbing only: one deterministic save per chunk
    python ingest.py --fresh    # force re-init even if a store exists
"""

import argparse
import json
import sys
from urllib.parse import unquote, urlsplit
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

MAX_OBJECT_CHARS = 48   # a fact object is a value, not a phrase
MAX_OBJECT_WORDS = 8


def slug_from_url(url):
    """`https://en.wikipedia.org/wiki/Haile_Gebrselassie` -> `Haile_Gebrselassie`.
    A short provenance label — never the raw URL (which can carry answer text)."""
    seg = urlsplit(url).path.rstrip("/").split("/")[-1]
    return unquote(seg) or url


def object_reject_reason(o):
    """Why this fact object is not atomic, or None if it is fine."""
    o = str(o).strip()
    if not o:
        return "empty object"
    if len(o) > MAX_OBJECT_CHARS:
        return f"object too long (>{MAX_OBJECT_CHARS} chars) — record an atomic value, not a phrase"
    if len(o.split()) > MAX_OBJECT_WORDS:
        return f"object has too many words (>{MAX_OBJECT_WORDS}) — record an atomic value, not a description"
    if "; " in o:
        return "object contains ';' — split into separate facts"
    return None


_NAME_ARTIFACTS = ("performance table", "reference titled", "dated reference",
                   "reference “", "titled “")


def name_reject_reason(name):
    """Why this element/subject name is a page-structure artifact rather than a
    real-world named thing, or None if it is fine. Long proper names (org/event
    names) are allowed; namespaced blobs ('Entity: performance table … row') and
    citations are not."""
    n = str(name).strip()
    low = n.lower()
    for m in _NAME_ARTIFACTS:
        if m in low:
            return f"name is a page-structure artifact ('{m.strip()}') — elements are real-world named things only"
    if ": " in n:
        return "name contains ':' (namespaced sub-item) — name the real entity and use a fact instead"
    if len(n) > 70:
        return "name too long to be a proper name — record a fact, not an element"
    return None


def ingest_recall_view(frame):
    """Tight view for the dedup loop: which focus terms resolved, and a few
    scored candidates for the ones that did not. Drops the score-0 cross-topic
    noise that made frames huge and expensive."""
    resolved = [f.get("name") for f in frame.get("focus", [])]
    candidates = {}
    for r in frame.get("resolution", []):
        if r.get("resolved"):
            continue
        cands = [{"name": c.get("name"), "score": round(c.get("score", 0), 2)}
                 for c in r.get("candidates", []) if c.get("score", 0) > 0][:5]
        if cands:
            candidates[r.get("submitted", "")] = cands
    view = {"resolved": resolved}
    if candidates:
        view["candidates"] = candidates
    return view


def make_dispatch(lg, src_label, journal, counters):
    def dispatch(name, args):
        if name == "legend_recall":
            frame = lg.recall({
                "focus": args.get("focus", []),
                "limit": args.get("limit", 10),
                "history_depth": args.get("history_depth", 1),
            })
            view = ingest_recall_view(frame)
            counters["recalls"] += 1
            journal.write({"event": "recall", "src": src_label, "args": args, "view": view})
            return json.dumps(view, ensure_ascii=False)

        if name == "legend_save":
            kept, rejected = [], []
            for f in args.get("facts", []):
                reason = name_reject_reason(f.get("s", "")) or object_reject_reason(f.get("o", ""))
                if reason:
                    rejected.append({"s": f.get("s"), "p": f.get("p"), "o": f.get("o"), "reason": reason})
                else:
                    f = dict(f)
                    f.setdefault("src", src_label)
                    kept.append(f)
            counters["rejected_facts"] += len(rejected)
            counters["kept_facts"] += len(kept)

            kept_elements, rejected_elements = [], []
            for el in args.get("elements", []):
                reason = name_reject_reason(el.get("name", ""))
                (rejected_elements if reason else kept_elements).append(
                    {**el, "reason": reason} if reason else el)
            counters["rejected_elements"] += len(rejected_elements)

            payload = {
                "source": src_label,
                "elements": kept_elements,
                "facts": kept,
                "changes": args.get("changes", []),
                "retract": args.get("retract", []),
                "merge": args.get("merge", []),
            }
            res = lg.save(payload)
            counters["saves"] += 1
            counters["minted_elements"] += len(res.get("writes", {}).get("minted_elements", []))
            counters["minted_relations"] += len(res.get("writes", {}).get("minted_relations", []))
            out = {
                "saved_facts": len(kept),
                "minted_elements": len(res.get("writes", {}).get("minted_elements", [])),
                "near_matches": res.get("near_matches", [])[:5],
                "conflicts": res.get("conflicts", [])[:5],
            }
            if rejected:
                out["rejected_facts"] = rejected
            if rejected_elements:
                out["rejected_elements"] = [{"name": e.get("name"), "reason": e.get("reason")} for e in rejected_elements]
            if rejected or rejected_elements:
                out["note"] = ("Some facts/elements were rejected (non-atomic object, or an "
                               "element name that is a table row / citation / description). "
                               "Elements are real-world named things; put values in atomic facts. "
                               "Re-submit only what is worth keeping, correctly shaped.")
            journal.write({"event": "save", "src": src_label, "kept": len(kept),
                          "rejected": rejected, "rejected_elements": rejected_elements, "result": out})
            return json.dumps(out, ensure_ascii=False)

        return json.dumps({"error": f"unknown tool {name}"})
    return dispatch


def ingest_page(lg, path, src_label, cfg, journal, counters, model):
    """Ingest one page's chunks through the Sol tool-loop. Shared by the real
    run and the single-page tuner so improvements transfer directly."""
    text = (d() / path).read_text(encoding="utf-8")
    chunks = chunk_text(text, cfg["ingest_chunk_tokens"])
    dispatch = make_dispatch(lg, src_label, journal, counters)
    for i, chunk in enumerate(chunks):
        user = (
            f"Reference page: {src_label}\n"
            f"Chunk {i + 1}/{len(chunks)}. Record only the atomic, durable facts this "
            f"chunk states (src={src_label!r}). Recall once, then save once.\n\n{chunk}"
        )
        try:
            res = oai.run_tool_loop(
                model=model, system=INGESTER_SYSTEM, user=user,
                tools=SAVE_TOOLS, dispatch=dispatch,
                max_calls=cfg["ingest_max_calls_per_chunk"], seed=cfg.get("seed"),
            )
            for k in ("prompt_tokens", "completion_tokens", "total_tokens"):
                counters["usage"][k] += res["usage_total"][k]
            journal.write({"event": "chunk_done", "src": src_label, "chunk": i,
                          "stop_reason": res["stop_reason"], "calls": res["tool_calls_made"],
                          "usage": res["usage_total"]})
        except Exception as e:
            journal.write({"event": "chunk_error", "src": src_label, "chunk": i,
                          "error": f"{type(e).__name__}: {e}"})
            print(f"  WARN chunk {i} of {src_label} errored: {e} — keeping partial saves")


def new_counters():
    return {"saves": 0, "recalls": 0, "minted_elements": 0, "minted_relations": 0,
            "kept_facts": 0, "rejected_facts": 0, "rejected_elements": 0,
            "usage": {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0}}


def pages_to_ingest(cfg):
    """Unique fetched pages for surviving questions, in first-seen manifest order.
    Returns list of (path, src_label)."""
    survivors = {q["qid"] for q in read_jsonl(d("corpus", "questions_supported.jsonl"))}
    seen, out = set(), []
    for m in read_jsonl(d("corpus", "manifest.jsonl")):
        if not m.get("ok") or m["qid"] not in survivors or m["path"] in seen:
            continue
        seen.add(m["path"])
        out.append((m["path"], slug_from_url(m["url"])))
    return out


def dry_dispatch_page(lg, path, src_label, journal, counters):
    """No-OpenAI plumbing test: one save per chunk from the first line of text."""
    text = (d() / path).read_text(encoding="utf-8")
    for i, chunk in enumerate(chunk_text(text, 2000)):
        title = next((ln.strip("# ").strip() for ln in chunk.splitlines() if ln.strip()), f"chunk{i}")
        res = lg.save({
            "source": src_label,
            "elements": [{"name": title[:60], "kind": "concept", "summary": "dry-run element"}],
            "facts": [{"s": title[:60], "p": "mentioned_in", "o": src_label[:40], "src": src_label}],
        })
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
    counters = new_counters()
    per_page_saves = {}
    model = cfg["models"]["ingester"]

    with Journal(d(JOURNAL)) as journal:
        for path, src_label in pages:
            if path in done:
                continue
            before = counters["saves"]
            journal.write({"event": "page_start", "path": path, "src": src_label})
            if args.dry_run:
                dry_dispatch_page(lg, path, src_label, journal, counters)
            else:
                ingest_page(lg, path, src_label, cfg, journal, counters, model)
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
        "recalls": counters["recalls"],
        "kept_facts": counters["kept_facts"],
        "rejected_facts": counters["rejected_facts"],
        "minted_elements": counters["minted_elements"],
        "minted_relations": counters["minted_relations"],
        "usage": counters["usage"],
        "per_page_saves": per_page_saves,
        "model": model,
        "dry_run": args.dry_run,
    }
    d(SUMMARY).write_text(json.dumps(summary, indent=2))
    print(json.dumps(summary, indent=2))

    zero_pages = [p for p, n in per_page_saves.items() if n == 0]
    if n_elements <= 32:
        print("FATAL: store has only the seed ontology — nothing was ingested.")
        sys.exit(1)
    if zero_pages:
        print(f"WARN: {len(zero_pages)} page(s) produced 0 saves: {zero_pages}")


if __name__ == "__main__":
    main()
