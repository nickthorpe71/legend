//! Top-level forward pass: token IDs → final 384-dim sentence
//! embedding (mean-pooled over non-padding tokens, L2-normalized).
//!
//! Public entry point: [`forward`].

use crate::inference::attention::Scratch;
use crate::inference::encoder::run_layer;
use crate::inference::ops::{l2_normalize_inplace, layernorm_inplace, masked_mean_pool};
use crate::inference::weights::Weights;

/// Run one full BERT forward pass for a single input sequence.
///
/// - `input_ids`: token IDs, length = `seq_len`. Must be ≤
///   `weights.max_position` (else the position embedding is out of
///   range and we panic).
/// - `attention_mask`: 1 for real tokens, 0 for padding, length = `seq_len`.
///
/// Returns the L2-normalized mean-pooled 384-dim sentence embedding.
pub fn forward(weights: &Weights, input_ids: &[u32], attention_mask: &[u32]) -> Vec<f32> {
    let seq_len = input_ids.len();
    assert_eq!(
        attention_mask.len(),
        seq_len,
        "input_ids/attention_mask length mismatch",
    );
    assert!(
        seq_len <= weights.max_position,
        "seq_len {seq_len} > max_position {}",
        weights.max_position,
    );
    let hidden = weights.hidden_size;

    // ── 1. Embedding lookup ───────────────────────────────────────
    // x[t, d] = word_emb[input_ids[t], d]
    //         + pos_emb[t, d]
    //         + type_emb[0, d]   (token_type_ids all 0 for our use)
    let mut x = vec![0.0f32; seq_len * hidden];
    let type0_row = &weights.type_emb[..hidden];
    for t in 0..seq_len {
        let tok = input_ids[t] as usize;
        assert!(tok < weights.vocab_size, "token id {tok} >= vocab_size");
        let word_row = &weights.word_emb[tok * hidden..(tok + 1) * hidden];
        let pos_row = &weights.pos_emb[t * hidden..(t + 1) * hidden];
        let dst = &mut x[t * hidden..(t + 1) * hidden];
        for d in 0..hidden {
            dst[d] = word_row[d] + pos_row[d] + type0_row[d];
        }
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
    let mask_f: Vec<f32> = attention_mask.iter().map(|&m| m as f32).collect();
    let mut scratch = Scratch::default();
    for layer in &weights.layers {
        run_layer(
            &mut x,
            seq_len,
            hidden,
            weights.intermediate_size,
            weights.num_heads,
            weights.head_dim,
            &mask_f,
            layer,
            weights.layer_norm_eps,
            &mut scratch,
        );
    }

    // ── 3. Mean-pool over real tokens ─────────────────────────────
    let mut pooled = masked_mean_pool(&x, seq_len, hidden, &mask_f);

    // ── 4. L2-normalize for cosine similarity in downstream code ──
    l2_normalize_inplace(&mut pooled);
    pooled
}
