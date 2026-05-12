//! Multi-head self-attention.
//!
//! For one input `x` of shape `[seq_len, hidden]`:
//! 1. Project to Q, K, V using `q_w, k_w, v_w` (each `[hidden, hidden]`).
//! 2. Reshape each as `[seq_len, num_heads, head_dim]` and treat as
//!    `num_heads` parallel attention heads.
//! 3. For each head: scores = (Q · K^T) / √head_dim, then apply the
//!    attention mask (add a large negative number to padding positions
//!    so they round to ~0 after softmax), then softmax over the keys
//!    axis, then attend = scores · V.
//! 4. Concat heads back to `[seq_len, hidden]` and project through
//!    `attn_out_w`.
//!
//! All buffers in this module are flat `Vec<f32>` row-major; head
//! reshape is implicit in the index arithmetic — no explicit
//! transpose, no extra allocation.

use crate::inference::ops::{add_bias_inplace, matmul, matmul_at_bt, softmax_inplace};
use crate::inference::weights::LayerWeights;

/// Run self-attention for one sequence. `mask` is `[seq_len]`,
/// 1.0 for real tokens, 0.0 for padding. Returns the attention
/// output (post `attn_out_w` projection) in a freshly allocated
/// `Vec<f32>` of length `seq_len * hidden`.
pub fn self_attention(
    x: &[f32],
    seq_len: usize,
    hidden: usize,
    num_heads: usize,
    head_dim: usize,
    mask: &[f32],
    layer: &LayerWeights,
    scratch: &mut Scratch,
) -> Vec<f32> {
    debug_assert_eq!(x.len(), seq_len * hidden);
    debug_assert_eq!(mask.len(), seq_len);
    debug_assert_eq!(num_heads * head_dim, hidden);

    // 1. Q, K, V projections — each [seq_len, hidden].
    scratch.q.clear();
    scratch.q.resize(seq_len * hidden, 0.0);
    matmul(seq_len, hidden, hidden, x, &layer.q_w, &mut scratch.q);
    add_bias_inplace(&mut scratch.q, &layer.q_b, seq_len, hidden);

    scratch.k.clear();
    scratch.k.resize(seq_len * hidden, 0.0);
    matmul(seq_len, hidden, hidden, x, &layer.k_w, &mut scratch.k);
    add_bias_inplace(&mut scratch.k, &layer.k_b, seq_len, hidden);

    scratch.v.clear();
    scratch.v.resize(seq_len * hidden, 0.0);
    matmul(seq_len, hidden, hidden, x, &layer.v_w, &mut scratch.v);
    add_bias_inplace(&mut scratch.v, &layer.v_b, seq_len, hidden);

    // 2. Per-head attention. Q, K, V are laid out as
    // `[seq_len, num_heads, head_dim]` (since the projection above
    // yields `[seq_len, hidden]` and hidden == num_heads * head_dim,
    // the natural reshape with strides `(hidden, head_dim, 1)` gives
    // exactly this layout). For each head h:
    //   q_h[t, d] = q[t * hidden + h * head_dim + d]
    // is a `[seq_len, head_dim]` view.
    let scale = 1.0 / (head_dim as f32).sqrt();
    let mut attn_concat = vec![0.0f32; seq_len * hidden];

    // Per-head scratch buffers — reused across heads.
    scratch.q_head.clear();
    scratch.q_head.resize(seq_len * head_dim, 0.0);
    scratch.k_head.clear();
    scratch.k_head.resize(seq_len * head_dim, 0.0);
    scratch.v_head.clear();
    scratch.v_head.resize(seq_len * head_dim, 0.0);
    scratch.scores.clear();
    scratch.scores.resize(seq_len * seq_len, 0.0);
    scratch.head_out.clear();
    scratch.head_out.resize(seq_len * head_dim, 0.0);

    // Pre-compute the additive mask: 0 for real tokens, large negative
    // for padding. Added to attention scores before softmax.
    let mask_bias: Vec<f32> = mask
        .iter()
        .map(|&m| if m > 0.0 { 0.0 } else { -1.0e9 })
        .collect();

    for h in 0..num_heads {
        // Gather q_head, k_head, v_head for this head: [seq_len, head_dim].
        for t in 0..seq_len {
            let src = t * hidden + h * head_dim;
            let dst = t * head_dim;
            scratch.q_head[dst..dst + head_dim].copy_from_slice(&scratch.q[src..src + head_dim]);
            scratch.k_head[dst..dst + head_dim].copy_from_slice(&scratch.k[src..src + head_dim]);
            scratch.v_head[dst..dst + head_dim].copy_from_slice(&scratch.v[src..src + head_dim]);
        }

        // scores[i, j] = q_head[i] · k_head[j], shape [seq_len, seq_len].
        // This is Q · K^T where Q = [seq_len, head_dim] and K = [seq_len, head_dim].
        matmul_at_bt(
            seq_len,
            head_dim,
            seq_len,
            &scratch.q_head,
            &scratch.k_head,
            &mut scratch.scores,
        );
        // Scale and apply mask. Mask is over KEY positions (the j axis).
        for i in 0..seq_len {
            let row = &mut scratch.scores[i * seq_len..(i + 1) * seq_len];
            for (j, s) in row.iter_mut().enumerate() {
                *s = *s * scale + mask_bias[j];
            }
        }
        // Softmax along keys axis (last axis).
        softmax_inplace(&mut scratch.scores, seq_len, seq_len);

        // head_out[i, d] = Σ_j scores[i, j] * v_head[j, d].
        // = scores · v_head, with scores [seq_len, seq_len] and
        // v_head [seq_len, head_dim] → head_out [seq_len, head_dim].
        matmul(
            seq_len,
            seq_len,
            head_dim,
            &scratch.scores,
            &scratch.v_head,
            &mut scratch.head_out,
        );

        // Scatter into the concat buffer at the head's slot.
        for t in 0..seq_len {
            let dst = t * hidden + h * head_dim;
            let src = t * head_dim;
            attn_concat[dst..dst + head_dim]
                .copy_from_slice(&scratch.head_out[src..src + head_dim]);
        }
    }

    // 4. Output projection: concat · attn_out_w + attn_out_b.
    let mut out = vec![0.0f32; seq_len * hidden];
    matmul(
        seq_len,
        hidden,
        hidden,
        &attn_concat,
        &layer.attn_out_w,
        &mut out,
    );
    add_bias_inplace(&mut out, &layer.attn_out_b, seq_len, hidden);
    out
}

/// Reusable per-tick scratch buffers. Allocated once at the top of the
/// forward pass, reused across all 6 layers. Saves a lot of allocation
/// pressure for the GC-less hot path.
#[derive(Default)]
pub struct Scratch {
    pub q: Vec<f32>,
    pub k: Vec<f32>,
    pub v: Vec<f32>,
    pub q_head: Vec<f32>,
    pub k_head: Vec<f32>,
    pub v_head: Vec<f32>,
    pub scores: Vec<f32>,
    pub head_out: Vec<f32>,
}
