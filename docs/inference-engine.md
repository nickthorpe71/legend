# Inference engine (INT8 BERT, pure Rust)

Legend's embedding path is a hand-rolled INT8 BERT forward pass that
runs all-MiniLM-L6-v2 with no C dependencies, no BLAS, no ONNX
runtime. ~1.7–2.0 ms per `embed_text` on a 13-token input
(i7-1365U, AVX-VNNI). This doc explains the pieces and the
non-obvious decisions.

Source-of-truth design lives in the code comments — this doc is the
bridge between files and the "why" behind the choices.

## Why pure Rust

We tried `ort` (ONNX Runtime via bindings) and `tract-onnx` earlier.
Both worked locally but bit us on portability:

- `ort` ships a dynamic library that doesn't cleanly cross between
  the native Linux box and a WSL2 instance on Windows — different
  shared-library expectations, GLIBC version mismatches, the usual.
- `tract-onnx` is pure Rust at runtime but pulls in `tract-hir` and
  `tract-core` as a giant transitive build-time dep tree (~30s cold
  builds, ~2400 μs per inference).

Owning the inference engine gets us:

1. **Portability**: one Rust binary, no `.so` / `.dll` to ship,
   identical behavior on WSL2 ↔ native Linux ↔ Windows.
2. **Speed**: ~17× faster than `tract-onnx` on the same model
   (30 ms → 1.8 ms) because we know we have exactly one model and
   can hand-tune the hot kernels.
3. **Binary size**: 27 MB stripped (was 108 MB with both fp32 and
   INT8 bundled, before we feature-gated the fp32 reference).

`tract-onnx` is now a dev-dependency only — used by
`examples/extract_weights.rs` to read the ONNX file once at
build-time and dump fp32 weights to `models/minilm-fp32.bin`. The
runtime library carries no tract dependency.

## Pipeline

```
embed.rs::embed_text(s)
 │
 │  HuggingFace tokenizers (pure Rust, .with_padding(None))
 ▼
[ids: Vec<u32>, mask: Vec<u32>]
 │
 │  bert_int8.rs::forward  (src/inference/bert_int8.rs:26)
 ▼
 ├─ Embedding lookup (per token)
 │    word + pos + type[0] → fused dequant + sum
 │    quantized_ops.rs:641  dequant_and_sum_token_embedding
 │
 ├─ Embedding LayerNorm
 │    ops.rs:96             layernorm_inplace
 │
 ├─ 6× encoder layer:        bert_int8.rs:181  run_layer_int8
 │   ├─ Q proj  (h→h, INT8)  quantized_ops.rs:259  quantized_matmul
 │   ├─ K proj  (h→h, INT8)        ↑ activation re-used from Q (input_token cache)
 │   ├─ V proj  (h→h, INT8)        ↑ activation re-used from Q
 │   ├─ Multi-head attention (fp32, strided over Q/K/V)
 │   │    bert_int8.rs:93/133  dot_fp32 + axpy_fp32
 │   │    ops.rs:362           softmax_inplace
 │   ├─ Attn output proj      (h→h, INT8)
 │   ├─ Residual add + LN
 │   ├─ FFN intermediate      (h→4h, INT8)
 │   ├─ GELU                  ops.rs:212  gelu_inplace
 │   ├─ FFN output            (4h→h, INT8)
 │   └─ Residual add + LN
 │
 └─ Masked mean pool + L2 normalize
      ops.rs:382/406
 ▼
Vec<f32> (384 dims, unit norm)
```

Hidden dim `h = 384`, intermediate `4h = 1536`, num_heads = 12,
head_dim = 64, 6 encoder layers. Model constants live in
`weights_int8.rs:WeightsInt8`.

## File map

```
src/inference/
├── mod.rs              ← module wiring + fp32_reference feature gate
├── bert_int8.rs        ← forward-pass orchestration (no SIMD itself)
├── ops.rs              ← fp32 kernels: matmul, LN, GELU, softmax, pool, L2
├── quantized_ops.rs    ← INT8 kernels: 3 matmul paths, activation quant,
│                         embedding dequant. The hot loop.
└── weights_int8.rs     ← LazyLock<WeightsInt8> over the bundled bin
```

The fp32 reference path (`bert.rs`, `attention.rs`, `encoder.rs`,
`weights.rs`) only compiles with `--features fp32_reference`. It's
the validation oracle for `examples/validate_int8.rs`; production
binaries don't carry it (saves 86 MB of weights from the binary).

## Quantization scheme

Two different quantizations, each chosen to match its tensor's
characteristics:

**Weights — symmetric, per-output-channel, INT8, static.**
Computed once at build time by `examples/quantize_to_int8.rs`. Each
weight matrix `W: [in_dim, out_dim]` is stored as
`QuantWeight { q_data: Vec<i8>, scales: Vec<f32> }` where
`scales[j]` is the per-column scale (one fp32 per output channel).
Dequantized lazily during matmul.

**Activations — symmetric, per-row (per-token), INT8, dynamic.**
Computed every forward in `quantize_activation`
(quantized_ops.rs:64). Each row of the activation matrix gets its
own scale, preserving per-token dynamic range. Two-pass: row max-abs
→ scale → quantize.

We tried per-tensor activation quant first (one scale for the whole
matrix) and got 0.965 cosine vs fp32 reference. Per-row gets us
0.996. The cost is `m` extra divisions + storing `m` scales — at
m=13, irrelevant.

The dequant formula at the end of every matmul is uniform:

```
out_fp32[i, j] = a_scale[i] · w_scale[j] · Σ_k a_i8[i, k] · w_i8[k, j]
                 + bias[j]
```

(see `quantized_matmul_prequant`, quantized_ops.rs:195)

## Column-major weight layout

Weights are stored as `[i8; in_dim * out_dim]` with **column j at
offsets j*in_dim..(j+1)*in_dim contiguously**. Set up in
`examples/quantize_to_int8.rs` (the transpose happens at build time)
and consumed by all three matmul kernels.

This is the whole point: every kernel reads one weight column at a
time as 32-byte contiguous chunks, which is exactly what
`_mm256_loadu_si256` and (more importantly) `vpdpbusd` want. If
weights were row-major, every load would be strided by `out_dim`.

## Three INT8 matmul kernels with runtime dispatch

`matmul_i8_dispatch` (quantized_ops.rs:281) picks the fastest path
at runtime via CPUID:

| Path               | Trigger                         | Speed                        | Code                 |
| ------------------ | ------------------------------- | ---------------------------- | -------------------- |
| `matmul_i8_vnni`   | AVX-VNNI (Alder Lake+ / Zen 4+) | ~3× faster than AVX2         | quantized_ops.rs:413 |
| `matmul_i8_avx2`   | AVX2 only                       | ~3× faster than scalar       | quantized_ops.rs:362 |
| `matmul_i8_scalar` | fallback                        | reference, obviously correct | quantized_ops.rs:309 |

The three kernels all produce the **same** `Σ a_i8 · w_i8` in
`c[i, j]`, so the dequant formula upstream is uniform — only the
hot loop differs.

### The +128 shift trick (VNNI)

`vpdpbusd` takes **unsigned** bytes × signed bytes. Our activations
are signed i8 in [-127, 127]. So we store activations twice in
`QMatmulScratch`:

- `act_i8: Vec<i8>` — for scalar / AVX2 paths
- `act_u8: Vec<u8>` — same values shifted by +128, for VNNI

After VNNI accumulates `Σ (a+128) · b`, we subtract the shift
correction:

```
Σ (a+128)·b = Σ a·b + 128·Σ b
            = (true dot)   + 128·col_sum
```

`col_sums[j]` is precomputed at weight load time
(`weights_int8.rs:QuantWeight.col_sums`) so the per-(i,j) cleanup
is one subtraction:

```rust
c[i * n + j] = sum - 128 * col_sums[j]
```

(see the tail of `matmul_i8_vnni`, e.g. quantized_ops.rs:511)

### 2×4 register tile

`matmul_i8_vnni` processes **2 rows × 4 columns at a time**. Inside
the inner loop:

- 8 ymm accumulators (one per output element in the 2×4 tile)
- 2 A-row loads + 4 B-column loads per k_chunk = 6 loads
- 8 `vpdpbusd` per k_chunk

Total: 14 ymm registers in flight, 2 free. AVX2 has 16 — fits.

Compared to a 1×4 tile, this reuses each B column **twice** (across
the two rows), halving the B-column load traffic in the hot path.
On i7-1365U this got matmul ~35% faster than 1×4. A 3×3 or 4×4 tile
would need more accumulators than fit.

Odd trailing row (when `m` is odd) drops through to a 1×4 tile —
see quantized_ops.rs:543.

### Why not just use AVX2 widen-and-pmaddwd everywhere?

`matmul_i8_avx2` (quantized_ops.rs:362) widens i8 → i16 and uses
`pmaddwd` to do 16-element multiply-add per instruction. It's a
universal AVX2 path. But VNNI's `vpdpbusd` is a _single_ instruction
for a 32-element INT8 dot product — 3× the work per cycle. When the
CPU has it, we use it; we keep the AVX2 path for older silicon.

## Activation cache (input_token)

Q, K, V projections all read the same post-LayerNorm activation. The
expensive part of `quantized_matmul` (quantized_ops.rs:259) is the
quantize-row step; the matmul itself is cheaper. So:

```rust
let tok = input_token(x, m, k);
if scratch.last_input_token != tok || scratch.a_scales.len() != m {
    quantize_activation(x, m, k, scratch);   // expensive — skipped for K, V
}
quantized_matmul_prequant(...);              // matmul against cached quant
```

`input_token` (quantized_ops.rs:235) hashes the first/last/middle
f32 values + length into a u64. False positives are astronomically
rare in the forward-pass hot path. False negatives just cost a
re-quantize, which is correct.

The cache hits cleanly for Q→K→V (same x) and misses correctly for
attn_out, FFN-int, FFN-out (different x).

## fp32 fast paths (ops.rs)

All of these dispatch to AVX2+FMA when available, scalar otherwise:

| Kernel                | Scalar                                          | AVX2                                               | Notes                                         |
| --------------------- | ----------------------------------------------- | -------------------------------------------------- | --------------------------------------------- |
| LayerNorm             | layernorm_inplace_scalar (ops.rs:121)           | layernorm_row_avx2 (ops.rs:154)                    | 3-pass: sum, var-sum, write-back              |
| GELU (exact erf form) | gelu_inplace_scalar (ops.rs:223)                | gelu_inplace_avx2 (ops.rs:293)                     | Uses vectorized exp + A&S 7.1.26 erf          |
| Activation quantize   | scalar loop in quantize_activation (qops.rs:64) | quantize_row_avx2 (qops.rs:112)                    | Two-pass: max-abs, then mul+round+pack        |
| Embedding dequant+sum | scalar fallback (qops.rs:641)                   | dequant_and_sum_token_embedding_avx2 (qops.rs:687) | Fused: word + pos + type → out, one SIMD pass |
| Attention dot product | scalar in dot_fp32 (bert_int8.rs:93)            | dot_fp32_avx2 (bert_int8.rs:110)                   | head_dim=64 → 8 fmadd chunks                  |
| Attention output mix  | scalar in axpy_fp32 (bert_int8.rs:133)          | axpy_fp32_avx2 (bert_int8.rs:149)                  | `out += scale · v`                            |

### AVX2 exp (the trick inside GELU)

`exp_avx2` (ops.rs:251) is the workhorse inside GELU's
`exp(-z²)`. Standard pattern:

```
x = n · ln(2) + r,  where n is the nearest integer
exp(x) = 2^n · exp(r)
```

- `r ∈ [-ln(2)/2, ln(2)/2]` is small enough that a 5th-order
  minimax polynomial gives ~2 ULP accuracy.
- `2^n` is computed by bit-manipulation: the IEEE-754 float
  `(n + 127) << 23` reinterpreted as f32 _is_ `2^n`. No actual
  exponentiation — just an integer add and a left shift.

GELU itself uses the **exact** erf formulation (not the tanh
approximation), preserving the math the model was trained with:

```
gelu(x) = 0.5 · x · (1 + erf(x / √2))
```

The erf is A&S 7.1.26, the same 7-term rational approximation as
the scalar fallback. Sign handling is one `vxorps` with the sign
bit.

## Inline strided attention

Attention is **not** routed through `ops::matmul`. The original
implementation copied each head's Q/K/V into contiguous
[seq, head_dim] scratch buffers and called `matrixmultiply::sgemm`
twice per head (Q·K^T and scores·V). For our shapes (m=13, k=64,
n=13), each sgemm has more setup overhead than actual work, and
the per-head slice copies added another ~30 μs.

Current implementation (bert_int8.rs:140–175 inside
`run_layer_int8`) walks the [seq, hidden] Q/K/V buffers directly
with strided pointer arithmetic:

```rust
for h in 0..num_heads {
    let head_off = h * head_dim;
    // scores[i, j] = Σ_d Q[i, head_off+d] · K[j, head_off+d]
    // ...
    softmax_inplace(...);
    // out[i, head_off+d] = Σ_j scores[i, j] · V[j, head_off+d]
}
```

Each inner dot/axpy is the AVX2 kernel above. No head copies, no
sgemm calls.

## Scratch buffers and zero-alloc steady state

`ScratchInt8` (bert_int8.rs:166) holds all the working buffers
needed during a forward pass:

```
qmat: QMatmulScratch      (act_i8, act_u8, acc, a_scales)
q, k, v: Vec<f32>         (each seq × hidden)
scores: Vec<f32>          (seq × seq)
attn_concat, attn_out
ffn_int, ffn_out
mask_f, mask_bias
```

A fresh `ScratchInt8::default()` allocates per call. Each `Vec`
is empty → first `resize` allocates the right size, subsequent
`resize` to the same size is a no-op. So the first forward in a
process pays ~14 small allocations; the cost is in noise relative
to the matmul work.

If we ever want batched inference or a long-lived embedder, the
scratch should hoist up to live alongside `WeightsInt8` and reset
each call. We don't bother today because we're already at the
memory-bandwidth ceiling (see next section).

## Performance ceiling

After all the SIMD work, the matmul kernels run at ~95% of AVX-VNNI
peak throughput (verified by counting `vpdpbusd` × clock and
comparing). Per `embed_text` at 13 tokens:

| Phase                             |        μs |        % |
| --------------------------------- | --------: | -------: |
| Matmul (weights)                  |     ~1100 |      62% |
| Unaccounted (memory-bound)        |      ~250 |      14% |
| Attention math (dot/axpy/softmax) |      ~150 |       8% |
| GELU                              |       ~60 |       3% |
| Activation quant                  |       ~45 |       3% |
| LayerNorm                         |       ~20 |       1% |
| Tokenize                          |       ~12 |       1% |
| **Total**                         | **~1750** | **100%** |

The "unaccounted" ~250 μs is the memory-bandwidth floor. Each
layer's weights total 1.77 MB (4 h→h projections + 2 FFN matrices),
which doesn't fit in L2 (1.25 MB on i7-1365U P-core). We stream
~10 MB of INT8 weights through cache per forward; at ~50 GB/s
effective bandwidth, that's ~200 μs of unavoidable memory transfer.

Further matmul micro-optimization is unlikely to help. The realistic
levers from here would be:

- **INT4 weights** (halve the bandwidth, ~2× speedup) — but quality
  risk, more quant-aware engineering.
- **Smaller model** — e.g. a 3-layer distillation. Architectural,
  not optimization.
- **Batched inference** — amortize weight loads across multiple
  sequences. Doesn't apply to online single-query.

## Portability

Runtime dispatch means one binary covers everything:

| CPU                                     | Path                                          | Speed                     |
| --------------------------------------- | --------------------------------------------- | ------------------------- |
| Alder Lake+ Intel (2021+) / Zen 4+ AMD  | VNNI                                          | full speed (~1.8 ms)      |
| Haswell+ Intel (2013+) / Excavator+ AMD | AVX2 widen-pmaddwd                            | ~3× slower matmul (~4 ms) |
| Pre-AVX2 x86_64                         | scalar                                        | ~10× slower, but works    |
| ARM (Apple Silicon, etc.)               | scalar via the `target_arch != x86_64` branch | works, no SIMD wins       |

ARM SIMD (NEON / SVE2) is not implemented — would be a separate
PR. The scalar fallback is correct everywhere.

## Validation

`examples/validate_int8.rs` (requires `--features fp32_reference`)
runs the same input through fp32 and INT8 paths and reports cosine
similarity. We hold at **0.996** vs fp32 reference for typical
inputs.

`cargo test` runs the kernel-vs-scalar parity tests:

```
cargo test --release --lib inference
```

The hot kernels each have a `*_matches_scalar` test that checks the
SIMD output bit-equals (or within 1 LSB / 5e-6 for non-integer
paths) the scalar reference across a sweep of dimensions and
edge-case alignments.

## Where the weights come from

```
models/all-MiniLM-L6-v2-q/model.onnx   (one-time download, ~23 MB)
 │
 │  cargo run --release --example extract_weights
 │     ├── tract-onnx loads the model
 │     ├── walks the topology by hardcoded MatMul IDs
 │     └── writes fp32 weights → models/minilm-fp32.bin (86 MB)
 ▼
models/minilm-fp32.bin
 │
 │  cargo run --release --example quantize_to_int8 --features fp32_reference
 │     ├── per-channel symmetric INT8 quant
 │     ├── column-major transpose
 │     ├── precompute col_sums for VNNI shift trick
 │     └── writes → models/minilm-int8.bin (22 MB)
 ▼
models/minilm-int8.bin
 │
 │  weights_int8.rs:86  include_bytes!()
 ▼
WeightsInt8 in the binary at runtime
```

Both `.bin` files are committed; you only re-run the extraction
pipeline if the upstream model changes or you change the quant
scheme.

## Where to read in the source

If you want to actually internalize the code, read in this order:

1. **`weights_int8.rs`** — what the data looks like on disk and in
   memory. The struct definitions tell you what every kernel will
   consume. (~5 min)
2. **`bert_int8.rs`** — `forward` + `run_layer_int8`. No math, just
   orchestration. Skip the AVX2 dot/axpy helpers on first pass. (~10 min)
3. **`ops.rs`** — read the scalar version of each kernel first
   (layernorm_inplace_scalar, gelu_inplace_scalar). Then the AVX2
   versions are minor variations. (~15 min)
4. **`quantized_ops.rs`** — the dense one. Start at
   `matmul_i8_scalar` (the reference). Then `matmul_i8_avx2` (widen
    - pmaddwd). Then `matmul_i8_vnni` (2×4 tile + shift trick). The
      rest is dispatch glue and activation quantize. (~30 min)
