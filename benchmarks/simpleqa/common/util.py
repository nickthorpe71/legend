"""Shared helpers: config loading, path anchoring, JSONL journal, chunking."""

import json
from pathlib import Path

HERE = Path(__file__).resolve().parent.parent  # benchmarks/simpleqa/


def load_config(path=None):
    path = Path(path) if path else HERE / "config.json"
    cfg = json.loads(path.read_text())
    cfg["_dir"] = str(HERE)
    return cfg


def d(*parts):
    """Path under benchmarks/simpleqa/."""
    return HERE.joinpath(*parts)


class Journal:
    """Append-only JSONL log. One event per line, flushed each write."""

    def __init__(self, path):
        self.path = Path(path)
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self._fh = self.path.open("a", encoding="utf-8")

    def write(self, event):
        self._fh.write(json.dumps(event, ensure_ascii=False) + "\n")
        self._fh.flush()

    def close(self):
        self._fh.close()

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()


def read_jsonl(path):
    path = Path(path)
    if not path.exists():
        return []
    out = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line:
            out.append(json.loads(line))
    return out


def write_jsonl(path, rows):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as fh:
        for r in rows:
            fh.write(json.dumps(r, ensure_ascii=False) + "\n")


def est_tokens(text):
    """Cheap token estimate (~4 chars/token). Good enough for chunking; we do
    not depend on a tokenizer that may not know the GPT-5.6 encoding."""
    return max(1, len(text) // 4)


def chunk_text(text, target_tokens):
    """Split on blank-line paragraph boundaries into ~target_tokens chunks.
    A single paragraph larger than the target becomes its own chunk rather than
    being cut mid-sentence."""
    paras = [p.strip() for p in text.split("\n\n") if p.strip()]
    chunks, cur, cur_tok = [], [], 0
    for p in paras:
        pt = est_tokens(p)
        if cur and cur_tok + pt > target_tokens:
            chunks.append("\n\n".join(cur))
            cur, cur_tok = [], 0
        cur.append(p)
        cur_tok += pt
    if cur:
        chunks.append("\n\n".join(cur))
    return chunks
