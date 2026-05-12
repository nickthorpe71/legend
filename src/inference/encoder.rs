//! One transformer encoder layer: self-attention + FFN, each with a
//! residual connection and a post-LayerNorm (BERT's Post-LN style).
//!
//! Sequence of ops, given input `x` of shape `[seq_len, hidden]`:
//!
//! ```text
//! attn_out = self_attention(x)
//! x        = layernorm(x + attn_out, attn_ln_gamma, attn_ln_beta)
//! ffn_int  = gelu(x @ ffn_int_w + ffn_int_b)        # [seq_len, intermediate]
//! ffn_out  = ffn_int @ ffn_out_w + ffn_out_b        # [seq_len, hidden]
//! x        = layernorm(x + ffn_out, ffn_ln_gamma, ffn_ln_beta)
//! ```

use crate::inference::attention::{self_attention, Scratch};
use crate::inference::ops::{
    add_bias_inplace, add_inplace, gelu_inplace, layernorm_inplace, matmul,
};
use crate::inference::weights::LayerWeights;

pub fn run_layer(
    x: &mut Vec<f32>,
    seq_len: usize,
    hidden: usize,
    intermediate: usize,
    num_heads: usize,
    head_dim: usize,
    mask: &[f32],
    layer: &LayerWeights,
    eps: f32,
    scratch: &mut Scratch,
) {
    debug_assert_eq!(x.len(), seq_len * hidden);

    // ── Self-attention + residual + LN ────────────────────────────
    let attn_out = self_attention(x, seq_len, hidden, num_heads, head_dim, mask, layer, scratch);
    add_inplace(x, &attn_out);
    layernorm_inplace(
        x,
        seq_len,
        hidden,
        &layer.attn_ln_gamma,
        &layer.attn_ln_beta,
        eps,
    );

    // ── FFN: intermediate (with GELU) → output ────────────────────
    let mut ffn_int = vec![0.0f32; seq_len * intermediate];
    matmul(seq_len, hidden, intermediate, x, &layer.ffn_int_w, &mut ffn_int);
    add_bias_inplace(&mut ffn_int, &layer.ffn_int_b, seq_len, intermediate);
    gelu_inplace(&mut ffn_int);

    let mut ffn_out = vec![0.0f32; seq_len * hidden];
    matmul(seq_len, intermediate, hidden, &ffn_int, &layer.ffn_out_w, &mut ffn_out);
    add_bias_inplace(&mut ffn_out, &layer.ffn_out_b, seq_len, hidden);

    add_inplace(x, &ffn_out);
    layernorm_inplace(
        x,
        seq_len,
        hidden,
        &layer.ffn_ln_gamma,
        &layer.ffn_ln_beta,
        eps,
    );
}
