//! INT8 forward pass for GLiNER2: embedding lookup + dequant, all
//! encoder matmuls via `quantized_matmul`, fp32 disentangled-attention
//! math, fp32 LayerNorms / GELU / softmax, BiLSTM in fp32 (small enough
//! that quantizing buys little), and INT8 span/prompt head MLPs.
//!
//! Structure mirrors `forward.rs` / `encoder.rs` / `head.rs` one-for-one
//! so the validation tests can run the same input through both paths
//! and diff at every checkpoint.

use crate::inference::deberta::decoding::{decode, generate_span_indices, PredictedEntity};
use crate::inference::deberta::weights_int8::{
    DebertaLayerInt8, ProjMlpInt8, WeightsDebertaInt8,
};
use crate::inference::ops::{
    add_bias_inplace, add_inplace, gelu_inplace, layernorm_inplace, softmax_inplace,
};
use crate::inference::quantized_ops::{quantized_matmul, QMatmulScratch};
use crate::inference::weights_int8::{QuantEmbedding, QuantWeight};

use crate::inference::deberta::rel_pos::build_relative_position_matrix;

// ── helpers ────────────────────────────────────────────────────────

/// Dequantize one row of a per-tensor INT8 embedding into the destination.
fn dequant_embedding_row(emb: &QuantEmbedding, row: usize, dst: &mut [f32]) {
    debug_assert_eq!(dst.len(), emb.cols);
    let src = &emb.q_data[row * emb.cols..(row + 1) * emb.cols];
    let scale = emb.scale;
    for (d, &q) in src.iter().enumerate() {
        dst[d] = (q as f32) * scale;
    }
}

/// Dequantize a whole per-tensor INT8 embedding table into `out`.
fn dequant_embedding_full(emb: &QuantEmbedding) -> Vec<f32> {
    let mut out = vec![0.0f32; emb.q_data.len()];
    let scale = emb.scale;
    for (i, &q) in emb.q_data.iter().enumerate() {
        out[i] = (q as f32) * scale;
    }
    out
}

/// INT8 linear: y = x @ W + b, fp32 in / fp32 out. Wraps
/// `quantized_matmul` so the call site doesn't have to thread the
/// scratch buffer around per matmul (each call allocates a scratch).
fn int8_linear(
    x: &[f32],
    w: &QuantWeight,
    b: &[f32],
    m: usize,
    k: usize,
    n: usize,
) -> Vec<f32> {
    let mut out = vec![0.0f32; m * n];
    let mut scratch = QMatmulScratch::default();
    quantized_matmul(x, m, k, n, w, b, &mut out, &mut scratch);
    out
}

#[inline]
fn relu_inplace(x: &mut [f32]) {
    for v in x.iter_mut() {
        if *v < 0.0 {
            *v = 0.0;
        }
    }
}

#[inline]
fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

// ── embedding ─────────────────────────────────────────────────────

pub fn embed_and_layernorm_int8(
    w: &WeightsDebertaInt8,
    input_ids: &[u32],
    attention_mask: &[u32],
) -> Vec<f32> {
    let seq_len = input_ids.len();
    let hidden = w.hidden_size;
    let mut x = vec![0.0f32; seq_len * hidden];
    for (t, &tok) in input_ids.iter().enumerate() {
        let row = &mut x[t * hidden..(t + 1) * hidden];
        dequant_embedding_row(&w.word_emb, tok as usize, row);
    }
    layernorm_inplace(
        &mut x,
        seq_len,
        hidden,
        &w.emb_ln_gamma,
        &w.emb_ln_beta,
        w.layer_norm_eps,
    );
    for t in 0..seq_len {
        if attention_mask[t] == 0 {
            for d in 0..hidden {
                x[t * hidden + d] = 0.0;
            }
        }
    }
    x
}

// ── attention (one layer) ─────────────────────────────────────────

/// Returns the per-head concatenated attention output `(seq_len, hidden)`.
#[allow(clippy::too_many_arguments)]
fn disentangled_attention_int8(
    x: &[f32],
    layer: &DebertaLayerInt8,
    rel_emb_lnd: &[f32],
    rel_pos_index: &[i32],
    attention_mask: &[u32],
    seq_len: usize,
    hidden: usize,
    num_heads: usize,
    head_dim: usize,
    rel_table_len: usize,
) -> Vec<f32> {
    // Q, K, V via INT8. Sharing scratch across the three calls lets the
    // input-token cache skip re-quantizing `x` for K and V.
    let mut q = vec![0.0f32; seq_len * hidden];
    let mut k = vec![0.0f32; seq_len * hidden];
    let mut v = vec![0.0f32; seq_len * hidden];
    {
        let mut scratch = QMatmulScratch::default();
        quantized_matmul(x, seq_len, hidden, hidden, &layer.q_w, &layer.q_b, &mut q, &mut scratch);
        quantized_matmul(x, seq_len, hidden, hidden, &layer.k_w, &layer.k_b, &mut k, &mut scratch);
        quantized_matmul(x, seq_len, hidden, hidden, &layer.v_w, &layer.v_b, &mut v, &mut scratch);
    }

    // Position projections — same Q/K weights applied to rel_emb_lnd.
    let mut q_pos = vec![0.0f32; rel_table_len * hidden];
    let mut k_pos = vec![0.0f32; rel_table_len * hidden];
    {
        let mut scratch = QMatmulScratch::default();
        quantized_matmul(
            rel_emb_lnd,
            rel_table_len,
            hidden,
            hidden,
            &layer.q_w,
            &layer.q_b,
            &mut q_pos,
            &mut scratch,
        );
        quantized_matmul(
            rel_emb_lnd,
            rel_table_len,
            hidden,
            hidden,
            &layer.k_w,
            &layer.k_b,
            &mut k_pos,
            &mut scratch,
        );
    }

    let scale = ((head_dim * 3) as f32).sqrt();
    let inv_scale = 1.0 / scale;

    let mut concat = vec![0.0f32; seq_len * hidden];
    let mut scratch_scores = vec![0.0f32; seq_len * seq_len];

    for h in 0..num_heads {
        let head_off = h * head_dim;

        // c2c.
        for qi in 0..seq_len {
            let q_row = &q[qi * hidden + head_off..qi * hidden + head_off + head_dim];
            for ki in 0..seq_len {
                let k_row = &k[ki * hidden + head_off..ki * hidden + head_off + head_dim];
                let mut dot = 0.0f32;
                for d in 0..head_dim {
                    dot += q_row[d] * k_row[d];
                }
                scratch_scores[qi * seq_len + ki] = dot * inv_scale;
            }
        }
        // c2p.
        for qi in 0..seq_len {
            let q_row = &q[qi * hidden + head_off..qi * hidden + head_off + head_dim];
            for ki in 0..seq_len {
                let idx = rel_pos_index[qi * seq_len + ki] as usize;
                let kp = &k_pos[idx * hidden + head_off..idx * hidden + head_off + head_dim];
                let mut dot = 0.0f32;
                for d in 0..head_dim {
                    dot += q_row[d] * kp[d];
                }
                scratch_scores[qi * seq_len + ki] += dot * inv_scale;
            }
        }
        // p2c.
        for qi in 0..seq_len {
            for ki in 0..seq_len {
                let k_row = &k[ki * hidden + head_off..ki * hidden + head_off + head_dim];
                let idx = rel_pos_index[qi * seq_len + ki] as usize;
                let qp = &q_pos[idx * hidden + head_off..idx * hidden + head_off + head_dim];
                let mut dot = 0.0f32;
                for d in 0..head_dim {
                    dot += qp[d] * k_row[d];
                }
                scratch_scores[qi * seq_len + ki] += dot * inv_scale;
            }
        }
        // Mask + softmax.
        for qi in 0..seq_len {
            for ki in 0..seq_len {
                if attention_mask[qi] == 0 || attention_mask[ki] == 0 {
                    scratch_scores[qi * seq_len + ki] = f32::MIN;
                }
            }
        }
        softmax_inplace(&mut scratch_scores, seq_len, seq_len);
        // V mix.
        for qi in 0..seq_len {
            let probs = &scratch_scores[qi * seq_len..(qi + 1) * seq_len];
            let out_row =
                &mut concat[qi * hidden + head_off..qi * hidden + head_off + head_dim];
            for (ki, &p) in probs.iter().enumerate() {
                if p == 0.0 {
                    continue;
                }
                let vrow = &v[ki * hidden + head_off..ki * hidden + head_off + head_dim];
                for d in 0..head_dim {
                    out_row[d] += p * vrow[d];
                }
            }
        }
    }

    concat
}

// ── one layer + full encoder ──────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn run_layer_int8(
    x: Vec<f32>,
    layer: &DebertaLayerInt8,
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
    let attn_concat = disentangled_attention_int8(
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

    let mut attn_out = int8_linear(&attn_concat, &layer.attn_out_w, &layer.attn_out_b, seq_len, hidden, hidden);
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

    let mut ffn_int = int8_linear(
        &post_attn,
        &layer.ffn_int_w,
        &layer.ffn_int_b,
        seq_len,
        hidden,
        intermediate,
    );
    gelu_inplace(&mut ffn_int);

    let mut ffn_out = int8_linear(
        &ffn_int,
        &layer.ffn_out_w,
        &layer.ffn_out_b,
        seq_len,
        intermediate,
        hidden,
    );
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

pub fn run_encoder_stack_int8(
    w: &WeightsDebertaInt8,
    mut x: Vec<f32>,
    attention_mask: &[u32],
) -> Vec<f32> {
    let seq_len = attention_mask.len();
    let hidden = w.hidden_size;
    let num_heads = w.num_heads;
    let head_dim = w.head_dim;
    let intermediate = w.intermediate_size;
    let rel_table_len = 2 * w.position_buckets;

    let rel_pos_index =
        build_relative_position_matrix(seq_len, w.position_buckets, w.max_position);

    // Dequantize rel_emb once, then layer-norm in fp32. Used by every layer.
    let mut rel_emb_lnd = dequant_embedding_full(&w.rel_emb);
    layernorm_inplace(
        &mut rel_emb_lnd,
        rel_table_len,
        hidden,
        &w.rel_emb_ln_gamma,
        &w.rel_emb_ln_beta,
        w.layer_norm_eps,
    );

    for layer in &w.layers {
        x = run_layer_int8(
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
            w.layer_norm_eps,
        );
    }
    x
}

// ── head pieces ───────────────────────────────────────────────────

pub fn project_tokens_int8(w: &WeightsDebertaInt8, encoder_out: &[f32], seq_len: usize) -> Vec<f32> {
    int8_linear(
        encoder_out,
        &w.proj_w,
        &w.proj_b,
        seq_len,
        w.hidden_size,
        w.projection_out,
    )
}

pub struct SplitOutputInt8 {
    pub words: Vec<f32>,
    pub prompts: Vec<f32>,
    pub num_words: usize,
    pub num_prompts: usize,
}

pub fn split_tokens_int8(
    w: &WeightsDebertaInt8,
    projected: &[f32],
    input_ids: &[u32],
    words_mask: &[u32],
    seq_len: usize,
) -> SplitOutputInt8 {
    let d = w.projection_out;
    let num_words = *words_mask.iter().max().unwrap_or(&0) as usize;
    let mut words = vec![0.0f32; num_words * d];
    for t in 0..seq_len {
        let wm = words_mask[t];
        if wm == 0 {
            continue;
        }
        let dst_word = (wm - 1) as usize;
        words[dst_word * d..(dst_word + 1) * d].copy_from_slice(&projected[t * d..(t + 1) * d]);
    }
    let mut prompts: Vec<f32> = Vec::new();
    let mut num_prompts = 0usize;
    for t in 0..seq_len {
        if input_ids[t] == w.class_token_index {
            prompts.extend_from_slice(&projected[t * d..(t + 1) * d]);
            num_prompts += 1;
        }
    }
    SplitOutputInt8 { words, prompts, num_words, num_prompts }
}

/// BiLSTM stays in fp32 — small footprint, sequential, marginal cost
/// dominated by other phases.
pub fn run_bilstm_int8(w: &WeightsDebertaInt8, words: &[f32], num_words: usize) -> Vec<f32> {
    let d = w.projection_out;
    let half = d / 2;
    let four_h = 4 * half;
    let mut out = vec![0.0f32; num_words * d];

    let run_dir = |ih_w: &[f32],
                   hh_w: &[f32],
                   ih_b: &[f32],
                   hh_b: &[f32],
                   reverse: bool|
     -> Vec<f32> {
        let mut hs = vec![0.0f32; num_words * half];
        let mut h_prev = vec![0.0f32; half];
        let mut c_prev = vec![0.0f32; half];
        let mut bias = vec![0.0f32; four_h];
        for (i, slot) in bias.iter_mut().enumerate() {
            *slot = ih_b[i] + hh_b[i];
        }
        let step_order: Vec<usize> = if reverse {
            (0..num_words).rev().collect()
        } else {
            (0..num_words).collect()
        };
        let mut gates = vec![0.0f32; four_h];
        for t in step_order {
            gates.fill(0.0);
            let x_row = &words[t * d..(t + 1) * d];
            for (j, &xv) in x_row.iter().enumerate() {
                let w_row = &ih_w[j * four_h..(j + 1) * four_h];
                for g in 0..four_h {
                    gates[g] += xv * w_row[g];
                }
            }
            for (j, &hv) in h_prev.iter().enumerate() {
                let w_row = &hh_w[j * four_h..(j + 1) * four_h];
                for g in 0..four_h {
                    gates[g] += hv * w_row[g];
                }
            }
            for g in 0..four_h {
                gates[g] += bias[g];
            }
            for dd in 0..half {
                let i = sigmoid(gates[dd]);
                let f = sigmoid(gates[half + dd]);
                let gg = gates[2 * half + dd].tanh();
                let o = sigmoid(gates[3 * half + dd]);
                let c = f * c_prev[dd] + i * gg;
                let h = o * c.tanh();
                c_prev[dd] = c;
                h_prev[dd] = h;
                hs[t * half + dd] = h;
            }
        }
        hs
    };

    let fwd = run_dir(&w.lstm_fwd.ih_w, &w.lstm_fwd.hh_w, &w.lstm_fwd.ih_b, &w.lstm_fwd.hh_b, false);
    let rev = run_dir(&w.lstm_rev.ih_w, &w.lstm_rev.hh_w, &w.lstm_rev.ih_b, &w.lstm_rev.hh_b, true);
    for t in 0..num_words {
        let dst = &mut out[t * d..(t + 1) * d];
        dst[..half].copy_from_slice(&fwd[t * half..(t + 1) * half]);
        dst[half..].copy_from_slice(&rev[t * half..(t + 1) * half]);
    }
    out
}

fn run_proj_mlp_int8(mlp: &ProjMlpInt8, x: &[f32], m: usize) -> Vec<f32> {
    let mut h = int8_linear(x, &mlp.lin1_w, &mlp.lin1_b, m, mlp.in_dim, mlp.inner_dim);
    relu_inplace(&mut h);
    int8_linear(&h, &mlp.lin2_w, &mlp.lin2_b, m, mlp.inner_dim, mlp.out_dim)
}

pub fn build_span_rep_int8(
    w: &WeightsDebertaInt8,
    words: &[f32],
    num_words: usize,
    spans: &[(usize, usize)],
) -> Vec<f32> {
    let d = w.projection_out;
    let start_rep = run_proj_mlp_int8(&w.project_start, words, num_words);
    let end_rep = run_proj_mlp_int8(&w.project_end, words, num_words);

    let n = spans.len();
    let mut cat = vec![0.0f32; n * 2 * d];
    for (i, &(s, e)) in spans.iter().enumerate() {
        let row = &mut cat[i * 2 * d..(i + 1) * 2 * d];
        row[..d].copy_from_slice(&start_rep[s * d..(s + 1) * d]);
        row[d..].copy_from_slice(&end_rep[e * d..(e + 1) * d]);
    }
    relu_inplace(&mut cat);
    run_proj_mlp_int8(&w.out_project, &cat, n)
}

pub fn project_prompts_int8(w: &WeightsDebertaInt8, prompts: &[f32], num_prompts: usize) -> Vec<f32> {
    run_proj_mlp_int8(&w.prompt, prompts, num_prompts)
}

// ── full pipeline (single entry point) ────────────────────────────

pub struct GlinerInt8Output {
    pub entities: Vec<PredictedEntity>,
}

/// End-to-end INT8 forward: input_ids → entities. `attention_mask` and
/// `words_mask` come from the GLiNER preprocessor (Phase 7 will wire
/// that up; for validation the oracle's fixtures supply them directly).
#[allow(clippy::too_many_arguments)]
pub fn predict_entities_int8(
    w: &WeightsDebertaInt8,
    input_ids: &[u32],
    attention_mask: &[u32],
    words_mask: &[u32],
    max_width: usize,
    threshold: f32,
) -> GlinerInt8Output {
    let seq_len = input_ids.len();
    let x = embed_and_layernorm_int8(w, input_ids, attention_mask);
    let enc = run_encoder_stack_int8(w, x, attention_mask);
    let projected = project_tokens_int8(w, &enc, seq_len);
    let split = split_tokens_int8(w, &projected, input_ids, words_mask, seq_len);
    let lstm = run_bilstm_int8(w, &split.words, split.num_words);
    let (spans, valid) = generate_span_indices(split.num_words, max_width);
    let span_rep = build_span_rep_int8(w, &lstm, split.num_words, &spans);
    let prompts = project_prompts_int8(w, &split.prompts, split.num_prompts);

    // Dot-product scoring (fp32).
    let d = w.projection_out;
    let n_spans = spans.len();
    let n_prompts = split.num_prompts;
    let mut scores = vec![0.0f32; n_spans * n_prompts];
    for s in 0..n_spans {
        let span_vec = &span_rep[s * d..(s + 1) * d];
        for c in 0..n_prompts {
            let prompt_vec = &prompts[c * d..(c + 1) * d];
            let mut dot = 0.0f32;
            for kk in 0..d {
                dot += span_vec[kk] * prompt_vec[kk];
            }
            scores[s * n_prompts + c] = dot;
        }
    }

    let entities = decode(&scores, &spans, &valid, n_prompts, threshold);
    GlinerInt8Output { entities }
}

// add_bias_inplace is imported but only used to keep parity with the
// fp32 path's ops set; reference once so the import isn't dead.
const _: fn(&mut [f32], &[f32], usize, usize) = add_bias_inplace;
