//! INT8 quantized BERT forward pass. Mirrors `bert.rs` (the fp32
//! reference) but routes every weight matmul through
//! `quantized_matmul`, which quantizes the activation dynamically,
//! does i8 × i8 → i32 matmul, dequantizes, and adds bias.
//!
//! LayerNorm, GELU, softmax, attention internals (Q·K^T,
//! attention·V), masked pooling and L2-normalize all stay fp32 — only
//! the 6 weight matmuls per layer are quantized.

// Same Rust 2024 friction as in `ops.rs` / `quantized_ops.rs`:
// `unsafe_op_in_unsafe_fn` demands explicit `unsafe` inside unsafe
// fns, then `unused_unsafe` complains about it. Silence the latter.
#![allow(unused_unsafe)]

use crate::inference::ops::{
    add_inplace, gelu_inplace, l2_normalize_inplace, layernorm_inplace, masked_mean_pool,
    softmax_inplace,
};
use crate::inference::quantized_ops::{
    QMatmulScratch, dequant_and_sum_token_embedding, quantized_matmul,
};
use crate::inference::weights_int8::{LayerWeightsInt8, WeightsInt8};

/// Same signature as [`crate::inference::bert::forward`] but uses
/// INT8 quantized weights.
pub fn forward(weights: &WeightsInt8, input_ids: &[u32], attention_mask: &[u32]) -> Vec<f32> {
    let seq_len = input_ids.len();
    assert_eq!(attention_mask.len(), seq_len);
    assert!(seq_len <= weights.max_position);
    let hidden = weights.hidden_size;

    // ── 1. Embedding lookup (dequantize on the fly) ───────────────
    let mut x = vec![0.0f32; seq_len * hidden];
    for t in 0..seq_len {
        let tok = input_ids[t] as usize;
        assert!(tok < weights.vocab_size);
        let dst = &mut x[t * hidden..(t + 1) * hidden];
        dequant_and_sum_token_embedding(
            &weights.word_emb.q_data,
            weights.word_emb.scale,
            tok,
            &weights.pos_emb.q_data,
            weights.pos_emb.scale,
            t,
            &weights.type_emb.q_data,
            weights.type_emb.scale,
            hidden,
            dst,
        );
    }
    layernorm_inplace(
        &mut x,
        seq_len,
        hidden,
        &weights.emb_ln_gamma,
        &weights.emb_ln_beta,
        weights.layer_norm_eps,
    );

    // ── 2. Encoder layers ─────────────────────────────────────────
    let mut scratch = ScratchInt8::default();
    scratch.mask_f.resize(seq_len, 0.0);
    scratch.mask_bias.resize(seq_len, 0.0);
    for (i, &m) in attention_mask.iter().enumerate() {
        let mf = m as f32;
        scratch.mask_f[i] = mf;
        scratch.mask_bias[i] = if mf > 0.0 { 0.0 } else { -1.0e9 };
    }
    for layer in &weights.layers {
        run_layer_int8(
            &mut x,
            seq_len,
            hidden,
            weights.intermediate_size,
            weights.num_heads,
            weights.head_dim,
            layer,
            weights.layer_norm_eps,
            &mut scratch,
        );
    }

    // ── 3. Mean pool + L2 normalize ───────────────────────────────
    let mut pooled = masked_mean_pool(&x, seq_len, hidden, &scratch.mask_f);
    l2_normalize_inplace(&mut pooled);
    pooled
}

/// Hot dot product for attention scoring. head_dim is 64 in MiniLM,
/// so 8 AVX2 chunks per call. Inlining + auto-vec from the compiler
/// is good here, but explicit AVX2 wins another ~30%.
#[inline(always)]
fn dot_fp32(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len());
    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma") {
            return unsafe { dot_fp32_avx2(a, b) };
        }
    }
    let mut s = 0.0;
    for i in 0..a.len() {
        s += a[i] * b[i];
    }
    s
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "fma")]
unsafe fn dot_fp32_avx2(a: &[f32], b: &[f32]) -> f32 {
    use std::arch::x86_64::*;
    let n = a.len();
    let chunks = n / 8;
    let tail_start = chunks * 8;
    let mut acc = unsafe { _mm256_setzero_ps() };
    for c in 0..chunks {
        let av = unsafe { _mm256_loadu_ps(a.as_ptr().add(c * 8)) };
        let bv = unsafe { _mm256_loadu_ps(b.as_ptr().add(c * 8)) };
        acc = unsafe { _mm256_fmadd_ps(av, bv, acc) };
    }
    let mut tmp = [0.0f32; 8];
    unsafe { _mm256_storeu_ps(tmp.as_mut_ptr(), acc) };
    let mut s = tmp.iter().sum::<f32>();
    for i in tail_start..n {
        s += a[i] * b[i];
    }
    s
}

/// `out += scale · v`. Used in attention output mixing where we
/// accumulate seq_len weighted V rows per output row.
#[inline(always)]
fn axpy_fp32(scale: f32, v: &[f32], out: &mut [f32]) {
    debug_assert_eq!(v.len(), out.len());
    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma") {
            unsafe { axpy_fp32_avx2(scale, v, out) };
            return;
        }
    }
    for i in 0..v.len() {
        out[i] += scale * v[i];
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "fma")]
unsafe fn axpy_fp32_avx2(scale: f32, v: &[f32], out: &mut [f32]) {
    use std::arch::x86_64::*;
    let n = v.len();
    let chunks = n / 8;
    let tail_start = chunks * 8;
    let s = unsafe { _mm256_set1_ps(scale) };
    for c in 0..chunks {
        let vv = unsafe { _mm256_loadu_ps(v.as_ptr().add(c * 8)) };
        let ov = unsafe { _mm256_loadu_ps(out.as_ptr().add(c * 8)) };
        let r = unsafe { _mm256_fmadd_ps(s, vv, ov) };
        unsafe { _mm256_storeu_ps(out.as_mut_ptr().add(c * 8), r) };
    }
    for i in tail_start..n {
        out[i] += scale * v[i];
    }
}

#[derive(Default)]
struct ScratchInt8 {
    qmat: QMatmulScratch, // shared scratch for the quantize→matmul→dequant cycle
    q: Vec<f32>,          // Q projection output
    k: Vec<f32>,          // K projection output
    v: Vec<f32>,          // V projection output
    scores: Vec<f32>,
    attn_concat: Vec<f32>,
    attn_out: Vec<f32>,
    ffn_int: Vec<f32>,
    ffn_out: Vec<f32>,
    mask_f: Vec<f32>,    // attention mask, fp32
    mask_bias: Vec<f32>, // additive softmax mask: 0 / -1e9
}

fn run_layer_int8(
    x: &mut Vec<f32>,
    seq_len: usize,
    hidden: usize,
    intermediate: usize,
    num_heads: usize,
    head_dim: usize,
    layer: &LayerWeightsInt8,
    eps: f32,
    scratch: &mut ScratchInt8,
) {
    debug_assert_eq!(x.len(), seq_len * hidden);

    // ── Q, K, V projections — quantized matmul ────────────────────
    scratch.q.resize(seq_len * hidden, 0.0);
    quantized_matmul(
        x,
        seq_len,
        hidden,
        hidden,
        &layer.q_w,
        &layer.q_b,
        &mut scratch.q,
        &mut scratch.qmat,
    );
    scratch.k.resize(seq_len * hidden, 0.0);
    quantized_matmul(
        x,
        seq_len,
        hidden,
        hidden,
        &layer.k_w,
        &layer.k_b,
        &mut scratch.k,
        &mut scratch.qmat,
    );
    scratch.v.resize(seq_len * hidden, 0.0);
    quantized_matmul(
        x,
        seq_len,
        hidden,
        hidden,
        &layer.v_w,
        &layer.v_b,
        &mut scratch.v,
        &mut scratch.qmat,
    );

    // ── Multi-head attention (fp32 — activation×activation, not a
    //    weight matmul, so quantization gains are negligible) ──────
    //
    // Strided directly over the [seq, hidden] Q/K/V buffers rather
    // than copying each head into a [seq, head_dim] scratch buffer.
    // The per-head copies cost ~25 ns each × 12K slices = ~320 μs we
    // don't pay anymore.
    let scale = 1.0 / (head_dim as f32).sqrt();
    scratch.attn_concat.clear();
    scratch.attn_concat.resize(seq_len * hidden, 0.0);
    scratch.scores.resize(seq_len * seq_len, 0.0);

    for h in 0..num_heads {
        let head_off = h * head_dim;
        // scores[i, j] = Σ_d Q[i, head_off+d] * K[j, head_off+d] · scale + mask_bias[j]
        for i in 0..seq_len {
            let q_row = &scratch.q[i * hidden + head_off..i * hidden + head_off + head_dim];
            let scores_row = &mut scratch.scores[i * seq_len..(i + 1) * seq_len];
            for j in 0..seq_len {
                let k_row = &scratch.k[j * hidden + head_off..j * hidden + head_off + head_dim];
                let dot = dot_fp32(q_row, k_row);
                scores_row[j] = dot * scale + scratch.mask_bias[j];
            }
        }
        softmax_inplace(&mut scratch.scores, seq_len, seq_len);
        // out[i, head_off+d] = Σ_j scores[i, j] * V[j, head_off+d]
        for i in 0..seq_len {
            let scores_row = &scratch.scores[i * seq_len..(i + 1) * seq_len];
            let out_row =
                &mut scratch.attn_concat[i * hidden + head_off..i * hidden + head_off + head_dim];
            for d in 0..head_dim {
                out_row[d] = 0.0;
            }
            for j in 0..seq_len {
                let s = scores_row[j];
                let v_row = &scratch.v[j * hidden + head_off..j * hidden + head_off + head_dim];
                axpy_fp32(s, v_row, out_row);
            }
        }
    }

    // ── Attention output projection (quantized) ───────────────────
    scratch.attn_out.resize(seq_len * hidden, 0.0);
    quantized_matmul(
        &scratch.attn_concat,
        seq_len,
        hidden,
        hidden,
        &layer.attn_out_w,
        &layer.attn_out_b,
        &mut scratch.attn_out,
        &mut scratch.qmat,
    );
    add_inplace(x, &scratch.attn_out);
    layernorm_inplace(
        x,
        seq_len,
        hidden,
        &layer.attn_ln_gamma,
        &layer.attn_ln_beta,
        eps,
    );

    // ── FFN intermediate + GELU + output (both quantized) ─────────
    scratch.ffn_int.resize(seq_len * intermediate, 0.0);
    quantized_matmul(
        x,
        seq_len,
        hidden,
        intermediate,
        &layer.ffn_int_w,
        &layer.ffn_int_b,
        &mut scratch.ffn_int,
        &mut scratch.qmat,
    );
    gelu_inplace(&mut scratch.ffn_int);

    scratch.ffn_out.resize(seq_len * hidden, 0.0);
    quantized_matmul(
        &scratch.ffn_int,
        seq_len,
        intermediate,
        hidden,
        &layer.ffn_out_w,
        &layer.ffn_out_b,
        &mut scratch.ffn_out,
        &mut scratch.qmat,
    );
    add_inplace(x, &scratch.ffn_out);
    layernorm_inplace(
        x,
        seq_len,
        hidden,
        &layer.ffn_ln_gamma,
        &layer.ffn_ln_beta,
        eps,
    );
}
