# Retrieval work session — 2026-07-22

Entry point / narrative index for a long day of retrieval work. Detail lives in the
linked docs; this ties the thread together so a future session can pick up here.

**One line:** shipped F1 (query→fact relevance ranking) + fixed an embed-off harness
bug → Legend **57% → 75%** on SimpleQA and the RAG gap **halved**; then, through five
independent dead-ends, converged on **over-extraction as the root cause** and
directly validated it; deferred the raw-passage anchor and F6 abstention with
evidence.

## The arc (chronological, with outcomes + detail docs)

1. **Six-perspective analysis of Legend** (science/product/systems/ML/skeptic/DX) →
   `docs/analysis-2026-07-21.md`. Converged on: the graph-vs-RAG comparison is the
   one unanswered question that gates everything.
2. **Ran the SimpleQA B-vs-D eval** (the experiment the review demanded) →
   `docs/eval-simpleqa-run-2026-07-22.md` (+ frozen artifacts in
   `eval-simpleqa-run-2026-07-22/`). First result: RAG dominated Legend by ~40 pts.
   Cost ~$68 (ingest ~$64 dominated — see the cost lesson in memory).
3. **Diagnosed recall** (`docs/retrieval-redesign.md`): recall does semantic *entity
   resolution* then dumps the neighborhood ranked by activation/recency, capped at
   10 — **no query→fact relevance step, no path-finding.** The embedding is spent on
   the entity and thrown away.
4. **Built F1** — `rerank_relevance` in `legend.c` + `embed_rank_texts` in `embed.c`
   (in-memory so recall stays read-only; gated on `embed_available()` so the
   `LEGEND_EMBED=0` determinism gate is untouched). Committed `f7b001f`.
5. **Found + fixed the embed-off harness bug** (same commit): the eval harness
   resolved the model dir relative to the benchmark CWD (absent) and never set
   `LEGEND_EMBED_DIR`, so **every arm-B recall in the original eval ran with
   embeddings OFF.** The store was embedded at ingest; recall never used it. Fixed
   `from_config` to pin `LEGEND_EMBED_DIR`. **Corrected result: arm B 57% → 75%;
   structure delta B−D_dense −40% → −22%; correct-and-store-grounded 18 → 30.** See
   the CORRECTION block atop the eval doc.
6. **Verified the trial is NOT affected** by the bug — its MCP server and all hooks
   pin `LEGEND_EMBED_DIR` to the absolute model dir; confirmed empirically on a store
   copy (model loads, candidates embedding-ranked). Note added to
   `docs/alchamancer-trial.md`.
7. **Broke down the remaining RAG gap** (13 D-only misses): ~8 ingest-miss (fact
   never stored / stored wrong), ~3–4 retrieval-miss (in store, not surfaced), **0
   consumption** (when a fact reaches the frame, the consumer uses it). Measurement
   lesson: short-factoid string grounding false-positives on "6"/"£20"/"United
   States" — verify object+subject, and measure at the deterministic frame level.
8. **Specced two levers**: raw-passage anchor (`docs/ingest-raw-anchor.md`) and F3
   traversal (`docs/retrieval-f3-traversal.md`).
9. **Adversarial panel (5 agents) reviewed the raw anchor** → **deferred**. The
   confidence gate is broken 4 ways (can't catch wrong-value misses; *fires more*
   in supersede/retract cases → resurrects buried values; BGE cosine band already
   measured dead by the trial; circular — reuses the score that made arm B never
   abstain); net-negative for revisable stores; flips ~3–4/8, ceiling ties RAG.
   Verdict block atop the raw-anchor doc.
10. **$2 no-code simulation** (`benchmarks/simpleqa/sim_raw_anchor.py`): **content
    works** (oracle-gate ceiling 7/8 — Terra readily uses handed passages, refuting
    "consumption is the bottleneck") but the **cosine gate is dead** (miss relevance
    overlaps hits). Viable only as opt-in always-on for immutable factoid corpora.
11. **F6 abstention** → **validated-and-deferred**: tested 3 abstention signals on
    the eval frames; **none cleanly separates** "graph has the answer" from "graph
    has a wrong neighbor" (same wall — the graph always holds a plausible fact).
12. **F3 diagnosis**: 447 is a *fuzzy-resolution* gap ("Yamraj" doesn't match the
    "Yamaraja" alias → 0 candidates), not traversal; and the store is a
    *pathological over-extraction swamp* (hundreds of junk `{rel:N, src:...}`
    relations; a single Pluto/Yama recall **timed out at 40s+**).
13. **Extraction test** (`benchmarks/simpleqa/ingest_subset.py` + `measure_tight.py`,
    ~$4.59, quota-truncated) → **over-extraction validated as a root cause**: cutting
    the ingester's "be exhaustive" clause **halved over-extraction (205 → 106
    elem/page)**, *fixed a wrong-value extraction* (2790 "Moral Sciences" →
    "Philosophy"), and **surfaced buried answers** (447 Pluto, 820 US equestrian).
    Caveat: the prompt over-corrected (dropped 2932/2952) → needs tuning.

## Headline results

- **F1 + embed-fix: Legend 57% → 75% on SimpleQA; RAG gap (B−D_dense) −40% → −22%.**
  F1's isolated retrieval contribution (deterministic frame-level): gold-in-frame
  42% → 60%, +11, 0 displaced.
- **Over-extraction is the confirmed root cause** — it manufactures the near-miss
  facts behind confident-wrong answers, makes abstention un-detectable, pollutes
  traversal, and blows up recall latency. Halving it fixed a wrong extraction and
  un-buried answers.

## Deferred (with evidence, so we don't relitigate)

- **Raw-passage anchor** — broken cosine gate; net-negative for revisable stores;
  content works but only viable as opt-in always-on for *immutable factoid corpora*.
  Behind an explicit factoid-store product decision. (`docs/ingest-raw-anchor.md`.)
- **F6 abstention (lexical/coverage)** — no clean separating signal on this data.
  (`docs/retrieval-redesign.md` §F6.)

## Open follow-ups (prioritized)

1. **Tune the extractor + full re-ingest** — the root everything traces to. The test
   prompt over-corrected; the fix is precision *without* dropping answer values. Full
   re-ingest (~$60) measures the confident-wrong reduction across all 60 and helps
   the live trial's store health. The expensive path, but the highest-value one.
2. **F1 latency optimization — DONE** (`2bc538d`). Bounded the fact-rerank with a
   cheap query-term-overlap pre-filter to the top `F1_RERANK_CAP=32` facts (lexical,
   not recency — F1 exists to surface a low-recency fact). Large-neighborhood recall
   **1.5s → 0.73–0.93s**, quality preserved (David Sweet date-of-birth still #1),
   gate green. Two things it surfaced, now tracked:
   - **2a. Fact-vector caching** — `embed_rank_texts` is uncached; fact vectors are
     *query-independent*, so embedding each once and reusing (like element vectors)
     would take repeat recalls well under 100ms. The 38s "F1" symptom was actually a
     cold `vectors.bin` (first recall embeds all elements); a save-time fact pre-warm
     closes both.
   - **2b. Non-F1 slow paths on polluted neighborhoods** — tier-2 element resolution
     on unresolved focus terms + frame assembly over hundreds of relations can still
     be slow (a Yama/Pluto recall timed out). Downstream of over-extraction (#B) —
     the clean-store fix helps here too.
3. **F5 fuzzy-resolution** — make "Yamraj" match the "Yamaraja" alias (447); cheap,
   recall-side. (`docs/retrieval-f3-traversal.md` §F5.)
4. **`embed.c` model-dir root cause** — it defaults the model dir relative to the
   process CWD, which is what caused the harness embed-off bug. Resolve it relative
   to the *binary's* location so no caller can trip it.
5. **Re-pin the trial** to a build with F1 once it's settled (the trial binary is
   pre-F1 `9d745ee`; its "rank-1-is-noise" findings were embeddings-on-but-recency).

## Artifact index

- **Docs:** `analysis-2026-07-21.md`, `eval-simpleqa-run-2026-07-22.md` (+CORRECTION),
  `retrieval-redesign.md` (F1–F6 catalog + all validation notes), `ingest-raw-anchor.md`
  (spec + adversarial verdict + sim), `retrieval-f3-traversal.md`, this file.
- **Frozen results:** `eval-simpleqa-run-2026-07-22/` (embed-off original) and
  `-corrected/` (embed-on+F1).
- **Scripts** (`benchmarks/simpleqa/`): `rerun_b.py` (arm-B-only re-run),
  `sim_raw_anchor.py` (the $2 sim), `ingest_subset.py` + `measure_tight.py`
  (extraction test).
- **Memory:** `project-simpleqa-b-vs-d-eval`, `project-retrieval-next-levers`
  (COME BACK TO THIS), `feedback-reasoning-model-cost-estimates`.
- **Commits:** `64dab57` analysis · `9bc3eb6` eval+arm-D · `f7b001f` F1+embed-fix ·
  `f997d4a` raw-anchor+F3 specs · `837d743` adversarial verdict · `44ef76b` sim ·
  `74c47e8` F6 validation · `4a6ff50` extraction test.

## Cost note

~$77 OpenAI this session; **credits ran out twice** (ingest is the driver — a
reasoning-model batch job costs far more than a smoke extrapolates). See
`feedback-reasoning-model-cost-estimates` in memory.
