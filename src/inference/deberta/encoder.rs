//! One DeBERTa-v3 encoder layer + the full-stack driver. Layer
//! structure:
//!
//! ```text
//! attn      = DisentangledSelfAttention(x)
//! hidden    = LayerNorm(attn_out_proj(attn) + x)
//! ffn_int   = GELU( hidden @ ffn_int_w + ffn_int_b )
//! ffn_out   = ffn_int @ ffn_out_w + ffn_out_b
//! layer_out = LayerNorm(ffn_out + hidden)
//! ```
//!
//! Both LayerNorms are eps=1e-7 (DeBERTa default).

use crate::inference::deberta::attention::{disentangled_self_attention, linear};
use crate::inference::deberta::rel_pos::build_relative_position_matrix;
use crate::inference::deberta::weights::WeightsDebertaV3;
use crate::inference::ops::{add_inplace, gelu_inplace, layernorm_inplace};

/// Apply one encoder layer to `x` in place (returns the new hidden).
/// `rel_pos_index` and `rel_emb_lnd` are computed once per forward
/// pass and reused across all layers.
#[allow(clippy::too_many_arguments)]
pub fn run_layer(
    x: Vec<f32>,
    layer: &crate::inference::deberta::weights::DebertaLayer,
    rel_emb_lnd: &[f32],
    rel_pos_index: &[i32],
    attention_mask: &[u32],
    seq_len: usize,
    hidden: usize,
    num_heads: usize,
    head_dim: usize,
    intermediate: usize,
    rel_table_len: usize,
    layer_norm_eps: f32,
) -> Vec<f32> {
    // 1. Disentangled self-attention -> concat over heads.
    let attn_concat = disentangled_self_attention(
        &x,
        layer,
        rel_emb_lnd,
        rel_pos_index,
        attention_mask,
        seq_len,
        hidden,
        num_heads,
        head_dim,
        rel_table_len,
    );

    // 2. Output projection.
    let mut attn_out = linear(
        &attn_concat,
        &layer.attn_out_w,
        &layer.attn_out_b,
        seq_len,
        hidden,
        hidden,
    );

    // 3. Add residual + LayerNorm.
    add_inplace(&mut attn_out, &x);
    layernorm_inplace(
        &mut attn_out,
        seq_len,
        hidden,
        &layer.attn_ln_gamma,
        &layer.attn_ln_beta,
        layer_norm_eps,
    );
    let post_attn = attn_out;

    // 4. FFN intermediate (Linear -> GELU).
    let mut ffn_int = linear(
        &post_attn,
        &layer.ffn_int_w,
        &layer.ffn_int_b,
        seq_len,
        hidden,
        intermediate,
    );
    gelu_inplace(&mut ffn_int);

    // 5. FFN output (Linear).
    let mut ffn_out = linear(
        &ffn_int,
        &layer.ffn_out_w,
        &layer.ffn_out_b,
        seq_len,
        intermediate,
        hidden,
    );

    // 6. Add residual + LayerNorm.
    add_inplace(&mut ffn_out, &post_attn);
    layernorm_inplace(
        &mut ffn_out,
        seq_len,
        hidden,
        &layer.ffn_ln_gamma,
        &layer.ffn_ln_beta,
        layer_norm_eps,
    );
    ffn_out
}

/// Full DeBERTa-v3 encoder stack: run all 6 layers in sequence,
/// returning `(seq_len, hidden)` row-major. `x` is the embedding output
/// (post-embedding-LN, padding-masked).
pub fn run_encoder_stack(
    weights: &WeightsDebertaV3,
    mut x: Vec<f32>,
    attention_mask: &[u32],
) -> Vec<f32> {
    let seq_len = attention_mask.len();
    let hidden = weights.hidden_size;
    let num_heads = weights.num_heads;
    let head_dim = weights.head_dim;
    let intermediate = weights.intermediate_size;
    let rel_table_len = 2 * weights.position_buckets;

    // 1. Pre-compute the shared rel-position bucket matrix and the
    //    layer-normed rel-embedding table. Same eps as the rest of the
    //    encoder.
    let rel_pos_index =
        build_relative_position_matrix(seq_len, weights.position_buckets, weights.max_position);
    let mut rel_emb_lnd = weights.rel_emb.clone();
    layernorm_inplace(
        &mut rel_emb_lnd,
        rel_table_len,
        hidden,
        &weights.rel_emb_ln_gamma,
        &weights.rel_emb_ln_beta,
        weights.layer_norm_eps,
    );

    // 2. Stack of encoder layers.
    for layer in &weights.layers {
        x = run_layer(
            x,
            layer,
            &rel_emb_lnd,
            &rel_pos_index,
            attention_mask,
            seq_len,
            hidden,
            num_heads,
            head_dim,
            intermediate,
            rel_table_len,
            weights.layer_norm_eps,
        );
    }

    x
}
