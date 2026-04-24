# Test Speed Optimization — Plan and Evidence

Authoritative brief for the optimization pass that follows the #03 baseline
recorded in `docs/baselines.md`. Driven by evidence from the 3-run release
test baseline on 2026-04-23 (logs in `.perf/cargo-test-release-2026-04-23.log`).

---

## Problem statement

`cargo test --release` warm-run wall time is **~98 min (5,918 s)** on a
12-core / 12-thread i7-1365U. User+sys time is 6,350 s, giving a parallelism
ratio of **1.07×**. On a 12-thread machine, effective parallelism of 1.07×
means the suite is running essentially single-threaded during test execution.

This is the single most actionable finding in the baseline. Everything else
(compile time, per-binary variance, thermal throttling) is rounding error
next to the fact that 11 of 12 cores sit idle whenever embeddings are
involved.

---

## Root cause — evidence chain

**The `SENTENCE_MODEL` mutex at `src/memory/entorhinal.rs:25`.**

```rust
static SENTENCE_MODEL: LazyLock<Mutex<SentenceModel>> = LazyLock::new(|| { ... });

pub fn embed_text(text: &str, dim: usize) -> Vec<f32> {
    ...
    let guard = SENTENCE_MODEL.lock().expect("sentence model mutex poisoned");
    let encoding = guard.tokenizer.encode(text, true)...;
    infer_embedding(&guard.model, &encoding, dim)
}
```

Every call to `embed_text` (and `embed_texts_batch`) takes the lock for the
entire tokenize + inference pass. The two giant unit-test binaries
(442 and 505 tests, accounting for 95% of warm wall time) run many tests in
parallel, each blocking on this mutex.

**The mutex is not required.** Upstream signatures:

- `tract_core::plan::SimplePlan::run(&self, inputs)` — takes `&self`;
  allocates a fresh `SimpleState` per call internally (per-run mutable
  state is owned by `SimpleState`, not `SimplePlan`).
- `tokenizers::tokenizer::Tokenizer::encode(&self, input, add_special_tokens)`
  — takes `&self`.

Neither method requires exclusive access. The `Mutex<SentenceModel>` forces
serialization for no functional reason — it was almost certainly a
precaution taken without checking the upstream types' actual thread-safety
contract.

Supporting evidence from the baseline:

- Warm runs: `user / real ≈ 1.07×` on 12 threads.
- The two embedding-light integration binaries (lexicon/bootstrap tests)
  finish quickly regardless of parallelism — they don't touch the mutex.
- Log shows "test has been running for over 60 seconds" repeatedly on
  tests in `memory::tests::*` (embedding-heavy) while unrelated
  non-embedding tests complete normally in the same window — exactly the
  signature of mutex-induced starvation, not CPU saturation.

---

## Optimizations — ranked by evidence-backed impact

### 1. Drop the mutex from `SENTENCE_MODEL`

**Change (sketch):**

```rust
// Before
static SENTENCE_MODEL: LazyLock<Mutex<SentenceModel>> = LazyLock::new(|| { ... Mutex::new(SentenceModel { ... }) });

pub fn embed_text(text: &str, dim: usize) -> Vec<f32> {
    ...
    let guard = SENTENCE_MODEL.lock().expect(...);
    let encoding = guard.tokenizer.encode(text, true).expect(...);
    infer_embedding(&guard.model, &encoding, dim)
}

// After
static SENTENCE_MODEL: LazyLock<SentenceModel> = LazyLock::new(|| SentenceModel { ... });

pub fn embed_text(text: &str, dim: usize) -> Vec<f32> {
    ...
    let encoding = SENTENCE_MODEL.tokenizer.encode(text, true).expect(...);
    infer_embedding(&SENTENCE_MODEL.model, &encoding, dim)
}
```

Apply the same transform to `embed_texts_batch` at `entorhinal.rs:232`.

**Expected impact:**
Parallelism ratio should climb from 1.07× to somewhere in the 6–10× range
(not a full 12× — thermal throttling caps sustained all-core throughput
on this CPU, per #02). Translates to warm test wall time in the
**10–20 min** range, down from 98 min. **~5–10× speedup**, completely in
the test-execution portion.

**Risk:** Low. Verify `SentenceModel` becomes auto-`Sync` (both fields —
`Tokenizer` and `TractModel` — need to be `Sync`). If the compiler accepts
the change, the types are thread-safe; if it rejects, the concrete
non-`Sync` field will be called out and we can wrap just that field (e.g.
in a `parking_lot::Mutex`, which is lighter than `std::sync::Mutex`) while
leaving the rest unlocked. Tract explicitly separates `SimplePlan` (the
read-only plan) from `SimpleState` (the per-run mutable state), so the
plan should be `Sync`.

**Verification plan (without re-running the full 98-min suite):**
- Cargo check — if Sync isn't derived, compilation fails with a specific
  field. That's our fast-fail signal.
- Run a single embedding-heavy unit test file in isolation
  (`cargo test --release -p legend memory::tests::test_diversity_prevents_merge_of_unrelated memory::tests::test_displacement_carries_l1_metadata_to_l2` — tests that
  appear in the "running for over 60 seconds" log). Measure wall time
  pre- and post-change on the same subset. If the subset drops ≥5×,
  the full suite almost certainly will too.
- Run a second embedding-light file (lexicon tests) to confirm
  behavioral parity — these should not change in any way.

**Do not** re-run the full suite until this optimization lands. The
single-test-file validation is sufficient signal to trust the change.

---

### 2. Fix the stale `insta` snapshot in `conformance_memory_workflows`

`tests/conformance_memory_workflows.rs:64` has an inline `insta::assert_json_snapshot!`
asserting a 10-memory shape. Commit `faa2fca` (ReadOnly retrieval) made
the new shape 1 memory. The test fails in all three baseline runs. Because
cargo stops at the first failing test binary, this also caused three
integration binaries (`conformance_cli`, `conformance_recovery`,
`observability_pre_phase_2`) to be skipped in every baseline run — their
numbers are not in the baseline.

**Fix:** update the inline snapshot literal to the new 1-memory shape (see
`.perf/cargo-test-release-2026-04-23.log` for the exact new content). Do
this alongside #1 so the post-optimization re-measurement captures all 14
binaries.

**Risk:** Zero. Same class of change as commit `660376f`.

---

### 3. Run test binaries in parallel via `cargo-nextest` (conditional)

Cargo runs test binaries sequentially by default. With #1 in place, within
a single binary we'll have good parallelism, but the 14 binaries still
stack sequentially. Most are small (<60s each), but cold compile + each
binary re-initializing the ONNX model adds up — roughly 30–60s of fixed
cost per binary just for tract `.into_optimized()` + tokenizer parse.

`cargo-nextest` runs test binaries concurrently (and tests within each
binary in parallel, like cargo test). On 14 binaries × 12 threads this
overlaps wall-clock considerably.

**Expected impact:** After #1, an additional **1.3–2×** wall-clock drop
(smaller integration binaries finish during the long unit-test binary's
runtime instead of queueing after it).

**Risk:** Medium. Requires `cargo install cargo-nextest` on the dev
machine + CI. Behavioral risks: integration tests in `tests/common/` use
`Harness::new()` which builds tmpdirs — if any of those paths collide
(unlikely but check), nextest surfaces the issue. Nextest also interprets
test output slightly differently; some `println!` debugging may move.

**Decide after #1 lands.** If post-#1 wall time is already <15 min, the
incremental value is small; save the complexity for later.

---

### 4. Verify, don't add: batch embedding call sites

`embed_texts_batch` exists and is more efficient than N calls to
`embed_text` when the caller has a collection. Post-#1, the mutex
serialization is gone, so `embed_texts_batch` loses most of its edge, but
it still reduces tokenizer setup overhead per call. Audit whether hot
test paths or production paths that build many embeddings in a tight loop
are using it; if not, switch them. Likely minor impact.

**Do not** do this speculatively — grep for `embed_text(` in loops and
benchmark specific offenders only.

---

### 5. Test-mode synthetic embedding — **skip unless #1 underperforms**

Was on the original list: a feature flag that replaces the ONNX forward
pass with a cheap deterministic hash → vector for tests that only need
vectors to be distinct and stable. **The evidence does not support doing
this.**

- Embedding cost per call is small; the mutex was making it look big.
  With the mutex gone, ONNX inference on a 23MB quantized MiniLM model is
  millisecond-scale per short input.
- Maintaining two embedding code paths creates real drift risk: dentate
  gyrus orthogonalization, thalamus salience, and neocortex similarity
  tests would all need to opt into the real path or risk silently
  testing fake behavior.
- Only reconsider if post-#1 wall time is still >30 min, at which point
  we have a very different bottleneck to find first.

---

### 6. Reduce test count — **not a priority**

220 tests in `src/memory/mod.rs` sounds like a lot, but each one asserts
a specific cognitive-stack behavior. The problem isn't that we have too
many tests — it's that they can't run in parallel. Fix #1 first; if the
result is satisfactory, leave the test surface alone.

---

## Success criteria

After #1 and #2 land and a fresh baseline is recorded (appended to
`docs/baselines.md` — do not overwrite), we should see:

- **Warm `cargo test --release` wall time**: ≤ 20 min (target), ≤ 25 min
  (acceptable). Down from 98 min.
- **Parallelism ratio (user+sys / real)**: ≥ 6.0× (target), ≥ 4.0×
  (acceptable). Up from 1.07×.
- **All 14 test binaries executing**: zero failed tests so the 3 currently
  skipped binaries (`conformance_cli`, `conformance_recovery`,
  `observability_pre_phase_2`) also contribute timing.
- **Behavioral parity**: all 1,010+ tests passing; no new flakiness.

If #1 alone hits the wall-time target, #3 stays un-done and we save the
complexity. If #1 undershoots (wall time in 25–40 min range), go to #3;
only if #1+#3 together undershoot do we consider #5.

---

## What *not* to do before #1

- Do not re-run the full `cargo test --release` suite. One re-run is a
  1.6-hour commitment; the optimization pass needs two re-measurements
  (pre-nextest, post-nextest) in the worst case. Burning one up front to
  "confirm the baseline" is wasteful given we already have 3 runs of
  data.
- Do not touch test code broadly. Changes to the embedding mutex are
  localized to `src/memory/entorhinal.rs`.
- Do not benchmark against #02's cold build number as a comparable. #02
  only compiled the main binary; #03 cold compiles 14 binaries.
