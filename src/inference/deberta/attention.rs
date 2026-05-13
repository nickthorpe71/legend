//! Disentangled self-attention for DeBERTa-v3.
//!
//! For each query position `q` and key position `k`, the attention
//! score is a sum of three terms (each scaled by `1 / sqrt(head_dim *
//! scale_factor)` where `scale_factor = 3` because we have `c2c +
//! c2p + p2c`):
//!
//! ```text
//! c2c = Q[q] · K[k]^T
//! c2p = Q[q] · K_pos[ bucket(q,k) ]^T
//! p2c = Q_pos[ bucket(k,q) ] · K[k]^T
//! ```
//!
//! `Q_pos` / `K_pos` come from the layer-normed relative-position
//! embeddings put through the SAME `query_proj` / `key_proj` as
//! content (because `share_att_key=True` in v0).
//!
//! Layout: we keep the `(seq, hidden)` row-major layout used by the
//! existing inference engine and walk heads via strided
//! pointer arithmetic — same pattern as `bert_int8::run_layer_int8`.

use crate::inference::deberta::weights::DebertaLayer;
use crate::inference::ops::softmax_inplace;

/// Output of disentangled self-attention: `(seq_len, hidden)` row-major.
/// `rel_emb_lnd` is the layer-normalized relative-position embedding
/// table, shape `[2 * position_buckets, hidden]`. `rel_pos_index` is
/// the `[seq_len * seq_len]` matrix of bucketed (q-k) indices in
/// `[0, 2*position_buckets)`.
#[allow(clippy::too_many_arguments)]
pub fn disentangled_self_attention(
    x: &[f32],
    layer: &DebertaLayer,
    rel_emb_lnd: &[f32],
    rel_pos_index: &[i32],
    attention_mask: &[u32],
    seq_len: usize,
    hidden: usize,
    num_heads: usize,
    head_dim: usize,
    rel_table_len: usize,
) -> Vec<f32> {
    debug_assert_eq!(x.len(), seq_len * hidden);
    debug_assert_eq!(rel_emb_lnd.len(), rel_table_len * hidden);
    debug_assert_eq!(rel_pos_index.len(), seq_len * seq_len);
    debug_assert_eq!(attention_mask.len(), seq_len);

    // Q, K, V from content: each (seq_len, hidden).
    let q = linear(x, &layer.q_w, &layer.q_b, seq_len, hidden, hidden);
    let k = linear(x, &layer.k_w, &layer.k_b, seq_len, hidden, hidden);
    let v = linear(x, &layer.v_w, &layer.v_b, seq_len, hidden, hidden);

    // Q_pos, K_pos: same projections applied to rel_emb_lnd.
    // Output shape (rel_table_len, hidden).
    let q_pos = linear(
        rel_emb_lnd,
        &layer.q_w,
        &layer.q_b,
        rel_table_len,
        hidden,
        hidden,
    );
    let k_pos = linear(
        rel_emb_lnd,
        &layer.k_w,
        &layer.k_b,
        rel_table_len,
        hidden,
        hidden,
    );

    // scale_factor = 3 (we have c2c + c2p + p2c). Scale = sqrt(head_dim
    // * scale_factor). Applied uniformly to all three terms so we can
    // fold it into a single divisor at the end.
    let scale = ((head_dim * 3) as f32).sqrt();
    let inv_scale = 1.0 / scale;

    // Build (q,k) -> mask. mask[q, k] = 1 iff attention_mask[q]==1 AND
    // attention_mask[k]==1. DeBERTaV2's get_attention_mask collapses
    // an (L,) vector to (L, L) by outer-AND.
    let pair_mask: Vec<u8> = {
        let mut m = vec![0u8; seq_len * seq_len];
        for q_i in 0..seq_len {
            for k_i in 0..seq_len {
                m[q_i * seq_len + k_i] = (attention_mask[q_i] & attention_mask[k_i]) as u8;
            }
        }
        m
    };

    // Build attention output (concat over heads then attn_out proj).
    let mut concat = vec![0.0f32; seq_len * hidden];
    let mut scratch_scores = vec![0.0f32; seq_len * seq_len];

    for h in 0..num_heads {
        let head_off = h * head_dim;

        // === step 1: scores[q, k] = c2c + c2p + p2c, all scaled.
        for q_i in 0..seq_len {
            let q_row = &q[q_i * hidden + head_off..q_i * hidden + head_off + head_dim];
            for k_i in 0..seq_len {
                let k_row = &k[k_i * hidden + head_off..k_i * hidden + head_off + head_dim];
                let mut dot = 0.0f32;
                for d in 0..head_dim {
                    dot += q_row[d] * k_row[d];
                }
                scratch_scores[q_i * seq_len + k_i] = dot * inv_scale;
            }
        }

        // === step 2: c2p_pos = clamp(rel_pos + pos_buckets, 0, 2*pb-1).
        // rel_pos_index *already* carries the +position_buckets shift
        // and clamp, so use it directly. For c2p, the lookup is into
        // K_pos at index rel_pos_index[q, k].
        // c2p_att[q, k] = Q[q] · K_pos[rel_pos_index[q, k]]^T  / scale
        for q_i in 0..seq_len {
            let q_row = &q[q_i * hidden + head_off..q_i * hidden + head_off + head_dim];
            for k_i in 0..seq_len {
                let idx = rel_pos_index[q_i * seq_len + k_i] as usize;
                let k_pos_row =
                    &k_pos[idx * hidden + head_off..idx * hidden + head_off + head_dim];
                let mut dot = 0.0f32;
                for d in 0..head_dim {
                    dot += q_row[d] * k_pos_row[d];
                }
                scratch_scores[q_i * seq_len + k_i] += dot * inv_scale;
            }
        }

        // === step 3: p2c. After HF's gather + transpose unwinds, the
        // contribution to scores[q, k] is
        //     K[k] · Q_pos[ p2c_pos[k, q] ] / scale
        // where p2c_pos[k, q] = clamp(-rel_pos[k, q] + pb, 0, ...).
        // Since log_bucket is odd, -log_bucket(k - q) = log_bucket(q - k),
        // so p2c_pos[k, q] = rel_pos_index[q, k] (the +pb shift is
        // already baked into our table).
        for q_i in 0..seq_len {
            for k_i in 0..seq_len {
                let k_row = &k[k_i * hidden + head_off..k_i * hidden + head_off + head_dim];
                let idx = rel_pos_index[q_i * seq_len + k_i] as usize;
                let q_pos_row =
                    &q_pos[idx * hidden + head_off..idx * hidden + head_off + head_dim];
                let mut dot = 0.0f32;
                for d in 0..head_dim {
                    dot += q_pos_row[d] * k_row[d];
                }
                scratch_scores[q_i * seq_len + k_i] += dot * inv_scale;
            }
        }

        // === step 4: mask + softmax.
        for q_i in 0..seq_len {
            for k_i in 0..seq_len {
                if pair_mask[q_i * seq_len + k_i] == 0 {
                    scratch_scores[q_i * seq_len + k_i] = f32::MIN;
                }
            }
        }
        softmax_inplace(&mut scratch_scores, seq_len, seq_len);

        // === step 5: out[q] += probs[q,:] · V[:, head_off..head_off+head_dim]
        for q_i in 0..seq_len {
            let probs_row = &scratch_scores[q_i * seq_len..(q_i + 1) * seq_len];
            let out_row =
                &mut concat[q_i * hidden + head_off..q_i * hidden + head_off + head_dim];
            for k_i in 0..seq_len {
                let prob = probs_row[k_i];
                if prob == 0.0 {
                    continue;
                }
                let v_row = &v[k_i * hidden + head_off..k_i * hidden + head_off + head_dim];
                for d in 0..head_dim {
                    out_row[d] += prob * v_row[d];
                }
            }
        }
    }

    concat
}

/// `y = x @ W + b`, plain f32. Convenience wrapper around
/// `ops::matmul` + `ops::add_bias_inplace`. Shape convention is m,k
/// for x; k,n for w; n for b; the result has shape m,n.
pub fn linear(x: &[f32], w: &[f32], b: &[f32], m: usize, k: usize, n: usize) -> Vec<f32> {
    use crate::inference::ops::{add_bias_inplace, matmul};
    let mut y = vec![0.0f32; m * n];
    matmul(m, k, n, x, w, &mut y);
    add_bias_inplace(&mut y, b, m, n);
    y
}
