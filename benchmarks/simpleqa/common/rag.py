"""Naive-RAG retrieval for arm D.

Two retrievers over the SAME corpus pages the Legend store was built from:
  - BM25   — Okapi lexical, zero-dependency (the "dumb retrieval" floor).
  - Dense  — OpenAI embeddings, cosine (a strong standard baseline).

Each returns the top passages whose cumulative token count fits a per-question
budget (matched to what arm B's recall injected), so arm D and arm B see the
same amount of context and the only difference is structured graph recall vs raw
chunk retrieval — design claim 3, "structure over dumb retrieval."
"""

import math
import re
from pathlib import Path

from common import oai
from common.util import chunk_text, est_tokens

_WORD = re.compile(r"[a-z0-9]+")


def _toks(s):
    return _WORD.findall(s.lower())


def load_chunks(pages_dir, chunk_tokens):
    """One flat list of retrieval chunks over every page, sorted by filename so
    the index is deterministic."""
    chunks = []
    for p in sorted(Path(pages_dir).glob("*.md")):
        text = p.read_text(encoding="utf-8")
        for i, c in enumerate(chunk_text(text, chunk_tokens)):
            chunks.append({"id": f"{p.name}#{i}", "page": p.name, "text": c,
                           "tokens": est_tokens(c)})
    return chunks


class BM25:
    """Okapi BM25 over the chunk corpus."""

    def __init__(self, chunks, k1=1.5, b=0.75):
        self.chunks = chunks
        self.k1, self.b = k1, b
        self.docs = [_toks(c["text"]) for c in chunks]
        self.tf = [{t: d.count(t) for t in set(d)} for d in self.docs]
        self.N = len(self.docs)
        self.avgdl = (sum(len(d) for d in self.docs) / self.N) if self.N else 0.0
        df = {}
        for d in self.docs:
            for t in set(d):
                df[t] = df.get(t, 0) + 1
        self.idf = {t: math.log(1 + (self.N - n + 0.5) / (n + 0.5)) for t, n in df.items()}

    def score(self, query):
        q = _toks(query)
        scores = []
        for i, d in enumerate(self.docs):
            dl = len(d)
            tf = self.tf[i]
            s = 0.0
            for t in q:
                f = tf.get(t, 0)
                if not f:
                    continue
                denom = f + self.k1 * (1 - self.b + self.b * dl / self.avgdl) if self.avgdl else 1.0
                s += self.idf.get(t, 0.0) * (f * (self.k1 + 1)) / denom
            scores.append(s)
        return scores


def _normalize(v):
    n = math.sqrt(sum(x * x for x in v)) or 1.0
    return [x / n for x in v]


class Dense:
    """Cosine over OpenAI embeddings. Embeds the whole corpus once at init."""

    def __init__(self, chunks, model):
        self.chunks = chunks
        self.model = model
        self.vecs = [_normalize(v) for v in oai.embed(model, [c["text"] for c in chunks])]

    def score(self, query):
        qv = _normalize(oai.embed(self.model, [query])[0])
        return [sum(a * b for a, b in zip(qv, v)) for v in self.vecs]


def top_to_budget(chunks, scores, budget_tokens):
    """Standard top-k-to-a-token-budget: take the highest-scored chunks until
    the cumulative token count reaches the budget (always at least one chunk)."""
    order = sorted(range(len(chunks)), key=lambda i: scores[i], reverse=True)
    picked, total = [], 0
    for i in order:
        picked.append(chunks[i])
        total += chunks[i]["tokens"]
        if total >= budget_tokens:
            break
    return picked
