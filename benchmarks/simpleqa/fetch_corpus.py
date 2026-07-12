#!/usr/bin/env python3
"""Step 2: build the corpus.

Deterministic sample of SimpleQA questions (filtered to Wikipedia sources),
dumb HTTP fetch of each source page, trafilatura extraction (full content,
tables kept so infoboxes survive), then a corpus-support audit: the gold answer
must appear on at least one of a question's pages, else the question is dropped
as unsupported. No LLM, no web-search — just scrape and extract.

Artifacts (all under corpus/):
  simple_qa_test_set.csv     cached dataset
  questions.jsonl            sampled questions (pre-audit)
  pages/<sha8>.md            extracted page text
  manifest.jsonl             one row per (qid, url) fetch attempt
  audit.json                 support decision per question
  questions_supported.jsonl  survivors (input to ingest/arms)
"""

import ast
import csv
import hashlib
import io
import json
import re
import sys
import urllib.request
from pathlib import Path

import trafilatura
from rapidfuzz import fuzz

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common.util import load_config, d, est_tokens  # noqa: E402

import random  # noqa: E402

AUDIT_FUZZ_MIN = 90  # partial_ratio threshold when exact substring misses

_MONTHS = {m: i for i, m in enumerate(
    ["january", "february", "march", "april", "may", "june", "july",
     "august", "september", "october", "november", "december"], start=1)}
_MONTH_RE = "|".join(_MONTHS)
_DATE_PATTERNS = [
    (re.compile(rf"\b({_MONTH_RE})\s+(\d{{1,2}}),?\s+(\d{{4}})\b", re.I), ("m", "d", "y")),
    (re.compile(rf"\b(\d{{1,2}})\s+({_MONTH_RE})\s+(\d{{4}})\b", re.I), ("d", "m", "y")),
    (re.compile(r"\b(\d{4})-(\d{2})-(\d{2})\b"), ("y", "mn", "d")),
]


def norm(s):
    return re.sub(r"\s+", " ", s or "").casefold().strip()


def dates_in(text):
    """Set of (year, month, day) tuples found in `text`, across common formats.
    Lets 'April 30, 2023' match '30 April 2023' while staying distinct from
    '1 May 2023'."""
    found = set()
    for rx, order in _DATE_PATTERNS:
        for m in rx.finditer(text):
            parts = dict(zip(order, m.groups()))
            try:
                y = int(parts["y"])
                mo = int(parts["mn"]) if "mn" in parts else _MONTHS[parts["m"].lower()]
                dnum = int(parts["d"])
                if 1 <= mo <= 12 and 1 <= dnum <= 31:
                    found.add((y, mo, dnum))
            except (KeyError, ValueError):
                continue
    return found


def sha8(s):
    return hashlib.sha256(s.encode("utf-8")).hexdigest()[:8]


def download_dataset(cfg):
    dst = d("corpus", "simple_qa_test_set.csv")
    dst.parent.mkdir(parents=True, exist_ok=True)
    if dst.exists() and dst.stat().st_size > 0:
        return dst
    req = urllib.request.Request(cfg["dataset_url"], headers={"User-Agent": cfg["http_user_agent"]})
    data = urllib.request.urlopen(req, timeout=120).read()
    dst.write_bytes(data)
    return dst


def canon_url(u):
    """Drop the #fragment. SimpleQA sometimes ships `#:~:text=<answer sentence>`
    anchors that quote the answer verbatim — ingesting those leaks the answer
    into the store. Stripping the fragment also collapses duplicate fetches of
    the same page."""
    return u.split("#", 1)[0]


def load_rows(csv_path):
    rows = []
    with csv_path.open(encoding="utf-8") as fh:
        for i, r in enumerate(csv.DictReader(fh)):
            md = ast.literal_eval(r["metadata"])  # single-quoted python literal
            urls = []
            for u in md.get("urls", []):
                u = canon_url(u)
                if u not in urls:
                    urls.append(u)
            rows.append(
                {
                    "qid": i,
                    "problem": r["problem"],
                    "answer": r["answer"],
                    "topic": md.get("topic"),
                    "answer_type": md.get("answer_type"),
                    "urls": urls,
                }
            )
    return rows


def sample(rows, cfg):
    pool = rows
    if cfg.get("wikipedia_only", True):
        pool = [r for r in rows if any("wikipedia.org" in u for u in r["urls"])]
    n = min(cfg["n_questions"], len(pool))
    idx = random.Random(cfg["seed"]).sample(range(len(pool)), n)
    picked = [pool[i] for i in sorted(idx)]
    # keep only the wikipedia URLs — that is the scrape scope
    for r in picked:
        r["urls"] = [u for u in r["urls"] if "wikipedia.org" in u] or r["urls"]
    return picked


_APPENDIX_RE = re.compile(
    r"^#+\s*(references|notes|external links|see also|further reading|"
    r"bibliography|citations|sources|footnotes)\b", re.I)


def strip_appendix(md):
    """Drop the trailing References / External links / Notes sections. They hold
    no answerable facts and are where the 'reference titled …' blob elements come
    from — often a third of a Wikipedia page."""
    out = []
    for line in md.splitlines():
        if _APPENDIX_RE.match(line.strip()):
            break
        out.append(line)
    return "\n".join(out).rstrip()


def fetch_and_extract(url, cfg):
    req = urllib.request.Request(url, headers={"User-Agent": cfg["http_user_agent"]})
    html = urllib.request.urlopen(req, timeout=60).read().decode("utf-8", errors="replace")
    text = trafilatura.extract(
        html,
        include_tables=True,       # Wikipedia infoboxes live in tables
        include_comments=False,
        include_links=False,
        favor_recall=True,
        output_format="markdown",
        url=url,
    )
    return strip_appendix(text) if text else text


def build_corpus(cfg):
    pages_dir = d("corpus", "pages")
    pages_dir.mkdir(parents=True, exist_ok=True)
    manifest, page_cache = [], {}

    questions = sample(load_rows(download_dataset(cfg)), cfg)
    (d("corpus", "questions.jsonl")).write_text(
        "".join(json.dumps(q, ensure_ascii=False) + "\n" for q in questions), encoding="utf-8"
    )
    print(f"sampled {len(questions)} question(s)")

    for q in questions:
        for url in q["urls"]:
            row = {"qid": q["qid"], "url": url}
            if url in page_cache:
                row.update(page_cache[url])
                manifest.append(row)
                continue
            try:
                text = fetch_and_extract(url, cfg)
                if not text or len(text.strip()) < 50:
                    raise ValueError("empty/too-short extraction")
                key = sha8(url)
                path = pages_dir / f"{key}.md"
                path.write_text(text, encoding="utf-8")
                info = {
                    "ok": True,
                    "sha256": hashlib.sha256(text.encode("utf-8")).hexdigest(),
                    "path": str(path.relative_to(d())),
                    "tokens": est_tokens(text),
                }
            except Exception as e:
                info = {"ok": False, "error": f"{type(e).__name__}: {e}"}
            page_cache[url] = info
            row.update(info)
            manifest.append(row)
            print(f"  qid={q['qid']} {url} -> {'ok ' + str(info.get('tokens')) + ' tok' if info['ok'] else 'FAIL ' + info['error']}")

    (d("corpus", "manifest.jsonl")).write_text(
        "".join(json.dumps(m, ensure_ascii=False) + "\n" for m in manifest), encoding="utf-8"
    )
    return questions, manifest, page_cache


def audit(cfg, questions, manifest):
    # map qid -> extracted page texts that fetched ok
    by_qid = {}
    for m in manifest:
        if m.get("ok"):
            by_qid.setdefault(m["qid"], []).append(d() / m["path"])

    decisions, survivors = [], []
    for q in questions:
        gold = norm(q["answer"]).strip(" .,;:!?'\"")  # trailing '2004.' -> '2004'
        gold_dates = dates_in(q["answer"])
        pages = by_qid.get(q["qid"], [])
        supported, how = False, "no_pages"
        for p in pages:
            raw = p.read_text(encoding="utf-8")
            page = norm(raw)
            if gold and gold in page:
                supported, how = True, "substring"
                break
            # a gold answer that is exactly one date matches on the day, any format
            if len(gold_dates) == 1 and gold_dates & dates_in(raw):
                supported, how = True, "date"
                break
            if gold and fuzz.partial_ratio(gold, page) >= AUDIT_FUZZ_MIN:
                supported, how = True, "fuzzy"
                break
        if pages and not supported:
            how = "answer_absent"
        decisions.append({"qid": q["qid"], "answer": q["answer"], "n_pages": len(pages),
                          "supported": supported, "how": how})
        if supported:
            survivors.append(q)

    (d("corpus", "audit.json")).write_text(json.dumps(decisions, indent=2, ensure_ascii=False), encoding="utf-8")
    (d("corpus", "questions_supported.jsonl")).write_text(
        "".join(json.dumps(q, ensure_ascii=False) + "\n" for q in survivors), encoding="utf-8"
    )
    return decisions, survivors


def main():
    cfg = load_config()
    questions, manifest, _ = build_corpus(cfg)
    decisions, survivors = audit(cfg, questions, manifest)

    n = len(questions)
    n_ok_fetch = len({m["qid"] for m in manifest if m.get("ok")})
    survival = len(survivors) / n if n else 0.0
    print(f"\nfetch: {n_ok_fetch}/{n} questions have >=1 page")
    print(f"audit: {len(survivors)}/{n} supported ({survival:.0%})")
    for dsn in decisions:
        if not dsn["supported"]:
            print(f"  DROP qid={dsn['qid']} ({dsn['how']}): {dsn['answer']!r}")

    if len(survivors) == 0:
        print("\nFATAL: no supported questions — nothing to ingest.")
        sys.exit(1)
    if n >= 10 and survival < cfg.get("audit_min_survival", 0.8):
        print(f"\nFATAL: survival {survival:.0%} < {cfg.get('audit_min_survival', 0.8):.0%} — "
              "inspect extraction (infobox/table loss?) before spending ingestion tokens.")
        sys.exit(1)
    if n < 10 and survival < cfg.get("audit_min_survival", 0.8):
        print(f"\nWARN: survival {survival:.0%} below target but n<10 (smoke) — continuing.")


if __name__ == "__main__":
    main()
