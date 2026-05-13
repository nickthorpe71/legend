//! Token embedding lookup + embedding LayerNorm for DeBERTa-v3.
//!
//! Differences from BERT:
//! - No absolute position embedding (`position_biased_input=False`).
//! - No token-type embedding (`type_vocab_size=0`).
//! - The LN applied here is `embeddings.LayerNorm`, with eps 1e-7.
//!
//! Pad positions are zeroed after the LN (matches HF
//! `DebertaV2Embeddings.forward` behaviour for non-`None` mask).

use crate::inference::deberta::weights::WeightsDebertaV3;
use crate::inference::ops::layernorm_inplace;

/// Run `word_embeddings(input_ids) -> LayerNorm -> mask`, in place
/// returning `(seq_len, hidden)` row-major.
pub fn embed_and_layernorm(
    weights: &WeightsDebertaV3,
    input_ids: &[u32],
    attention_mask: &[u32],
) -> Vec<f32> {
    let seq_len = input_ids.len();
    let hidden = weights.hidden_size;
    assert_eq!(
        attention_mask.len(),
        seq_len,
        "input_ids/attention_mask length mismatch",
    );

    let mut x = vec![0.0f32; seq_len * hidden];
    for (t, &tok) in input_ids.iter().enumerate() {
        let tok = tok as usize;
        assert!(
            tok < weights.vocab_size,
            "token id {tok} >= vocab_size {}",
            weights.vocab_size,
        );
        let src = &weights.word_emb[tok * hidden..(tok + 1) * hidden];
        let dst = &mut x[t * hidden..(t + 1) * hidden];
        dst.copy_from_slice(src);
    }

    layernorm_inplace(
        &mut x,
        seq_len,
        hidden,
        &weights.emb_ln_gamma,
        &weights.emb_ln_beta,
        weights.layer_norm_eps,
    );

    // Zero out padded positions. For Legend's single-tick inputs this
    // is usually a no-op (every position is real) but it makes the
    // output match the oracle exactly when padding is present.
    for t in 0..seq_len {
        if attention_mask[t] == 0 {
            for d in 0..hidden {
                x[t * hidden + d] = 0.0;
            }
        }
    }

    x
}
