//! GLiNER head: everything after the DeBERTa encoder. Splits the
//! per-token output into per-word + per-prompt embeddings, runs the
//! BiLSTM on words, computes span representations (markerV0), projects
//! prompt embeddings, scores spans × prompts via dot product, and
//! decodes the top-scoring entity spans with flat-NER non-overlap.
//!
//! Validated against `oracle/fixtures/dentist/*.bin` end-to-end.

use crate::inference::deberta::attention::linear;
use crate::inference::deberta::weights::{LstmDirection, ProjMlp, WeightsDebertaV3};

// ---------------------------------------------------------------------
// Activations: ReLU and the LSTM gates (sigmoid + tanh).
// ---------------------------------------------------------------------

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

// ---------------------------------------------------------------------
// 1. Projection 768 -> 512 (applied to every token in the encoder out).
// ---------------------------------------------------------------------

/// Project encoder output `(seq_len, hidden=768)` to `(seq_len, 512)`.
pub fn project_tokens(weights: &WeightsDebertaV3, encoder_out: &[f32], seq_len: usize) -> Vec<f32> {
    linear(
        encoder_out,
        &weights.proj_w,
        &weights.proj_b,
        seq_len,
        weights.hidden_size,
        weights.projection_out,
    )
}

// ---------------------------------------------------------------------
// 2. Word / prompt split. words_mask[t] > 0 marks the *first* subword of
//    word index (words_mask[t] - 1). input_ids[t] == class_token_index
//    marks one prompt token; the i-th match goes to prompts[i].
// ---------------------------------------------------------------------

pub struct SplitOutput {
    /// `(num_words, projection_out)` row-major.
    pub words: Vec<f32>,
    /// `(num_prompts, projection_out)` row-major.
    pub prompts: Vec<f32>,
    pub num_words: usize,
    pub num_prompts: usize,
}

pub fn split_tokens(
    weights: &WeightsDebertaV3,
    projected: &[f32],
    input_ids: &[u32],
    words_mask: &[u32],
    seq_len: usize,
) -> SplitOutput {
    let d = weights.projection_out;
    debug_assert_eq!(projected.len(), seq_len * d);
    debug_assert_eq!(words_mask.len(), seq_len);
    debug_assert_eq!(input_ids.len(), seq_len);

    // Words: max index in words_mask = num_words; values are 1-indexed.
    let num_words = *words_mask.iter().max().unwrap_or(&0) as usize;
    let mut words = vec![0.0f32; num_words * d];
    for t in 0..seq_len {
        let w = words_mask[t];
        if w == 0 {
            continue;
        }
        let dst_word = (w - 1) as usize;
        let src = &projected[t * d..(t + 1) * d];
        let dst = &mut words[dst_word * d..(dst_word + 1) * d];
        dst.copy_from_slice(src);
    }

    // Prompts: positions where input_ids == class_token_index. We use
    // those positions' projected embeddings directly (embed_ent_token=True).
    let mut prompts: Vec<f32> = Vec::new();
    let mut num_prompts = 0usize;
    for t in 0..seq_len {
        if input_ids[t] == weights.class_token_index {
            prompts.extend_from_slice(&projected[t * d..(t + 1) * d]);
            num_prompts += 1;
        }
    }

    SplitOutput {
        words,
        prompts,
        num_words,
        num_prompts,
    }
}

// ---------------------------------------------------------------------
// 3. BiLSTM. PyTorch packs (i, f, g, o) along dim 0; weights are stored
//    as [in_dim, 4 * hidden]. For one direction at step t:
//
//        gates = x_t @ ih_w + ih_b + h_{t-1} @ hh_w + hh_b
//        i,f,g,o = split gates into 4 slices of `hidden`
//        c_t   = sigmoid(f) * c_{t-1} + sigmoid(i) * tanh(g)
//        h_t   = sigmoid(o) * tanh(c_t)
//
//    Output stacks forward h then reverse h along the last axis to
//    give shape (seq, 2 * hidden_half) = (seq, projection_out).
// ---------------------------------------------------------------------

fn run_lstm_direction(
    dir: &LstmDirection,
    inputs: &[f32],
    seq_len: usize,
    input_dim: usize,
    hidden_half: usize,
    reverse: bool,
) -> Vec<f32> {
    let four_h = 4 * hidden_half;
    let mut out = vec![0.0f32; seq_len * hidden_half];
    let mut h_prev = vec![0.0f32; hidden_half];
    let mut c_prev = vec![0.0f32; hidden_half];

    // Pre-compute combined bias once.
    let mut bias = vec![0.0f32; four_h];
    for (i, slot) in bias.iter_mut().enumerate() {
        *slot = dir.ih_b[i] + dir.hh_b[i];
    }

    let step_order: Vec<usize> = if reverse {
        (0..seq_len).rev().collect()
    } else {
        (0..seq_len).collect()
    };

    let mut gates = vec![0.0f32; four_h];

    for t in step_order {
        // gates = x_t @ ih_w (1 x in_dim · in_dim x 4h → 1 x 4h)
        gates.fill(0.0);
        let x_row = &inputs[t * input_dim..(t + 1) * input_dim];
        for (d, &xv) in x_row.iter().enumerate() {
            let w_row = &dir.ih_w[d * four_h..(d + 1) * four_h];
            for g in 0..four_h {
                gates[g] += xv * w_row[g];
            }
        }
        // gates += h_prev @ hh_w
        for (d, &hv) in h_prev.iter().enumerate() {
            let w_row = &dir.hh_w[d * four_h..(d + 1) * four_h];
            for g in 0..four_h {
                gates[g] += hv * w_row[g];
            }
        }
        // + bias.
        for g in 0..four_h {
            gates[g] += bias[g];
        }

        // Split into i, f, g, o per torch's gate order.
        let (i_part, rest) = gates.split_at(hidden_half);
        let (f_part, rest) = rest.split_at(hidden_half);
        let (g_part, o_part) = rest.split_at(hidden_half);

        for d in 0..hidden_half {
            let i = sigmoid(i_part[d]);
            let f = sigmoid(f_part[d]);
            let g = g_part[d].tanh();
            let o = sigmoid(o_part[d]);
            let c = f * c_prev[d] + i * g;
            let h = o * c.tanh();
            c_prev[d] = c;
            h_prev[d] = h;
            out[t * hidden_half + d] = h;
        }
    }
    out
}

/// One-layer bidirectional LSTM. Input shape `(seq_len, projection_out)`,
/// output shape `(seq_len, projection_out)` (forward+reverse concatenated).
pub fn run_bilstm(weights: &WeightsDebertaV3, words: &[f32], num_words: usize) -> Vec<f32> {
    let d = weights.projection_out;
    let half = d / 2;
    let fwd = run_lstm_direction(&weights.lstm_fwd, words, num_words, d, half, false);
    let rev = run_lstm_direction(&weights.lstm_rev, words, num_words, d, half, true);
    let mut out = vec![0.0f32; num_words * d];
    for t in 0..num_words {
        let dst = &mut out[t * d..(t + 1) * d];
        dst[..half].copy_from_slice(&fwd[t * half..(t + 1) * half]);
        dst[half..].copy_from_slice(&rev[t * half..(t + 1) * half]);
    }
    out
}

// ---------------------------------------------------------------------
// 4. Span representation (markerV0).
// ---------------------------------------------------------------------

/// Apply the two-layer MLP `lin2(relu(lin1(x)))` row-wise. Input is
/// `(m, in_dim)`; output is `(m, out_dim)`.
pub fn run_proj_mlp(mlp: &ProjMlp, x: &[f32], m: usize) -> Vec<f32> {
    let mut h = linear(x, &mlp.lin1_w, &mlp.lin1_b, m, mlp.in_dim, mlp.inner_dim);
    relu_inplace(&mut h);
    linear(&h, &mlp.lin2_w, &mlp.lin2_b, m, mlp.inner_dim, mlp.out_dim)
}

/// Build span representations `(num_words * max_width, projection_out)`
/// using markerV0: concat(start_proj, end_proj) → ReLU → out_project.
/// `words` is the BiLSTM output, shape `(num_words, projection_out)`.
pub fn build_span_rep(
    weights: &WeightsDebertaV3,
    words: &[f32],
    num_words: usize,
    spans: &[(usize, usize)],
) -> Vec<f32> {
    let d = weights.projection_out;
    let start_rep = run_proj_mlp(&weights.project_start, words, num_words);
    let end_rep = run_proj_mlp(&weights.project_end, words, num_words);

    let n = spans.len();
    let mut cat = vec![0.0f32; n * 2 * d];
    for (i, &(s, e)) in spans.iter().enumerate() {
        let row = &mut cat[i * 2 * d..(i + 1) * 2 * d];
        row[..d].copy_from_slice(&start_rep[s * d..(s + 1) * d]);
        row[d..].copy_from_slice(&end_rep[e * d..(e + 1) * d]);
    }
    relu_inplace(&mut cat);
    run_proj_mlp(&weights.out_project, &cat, n)
}

// ---------------------------------------------------------------------
// 5. Prompt projection + scoring + decoding.
// ---------------------------------------------------------------------

/// Apply the prompt MLP to label embeddings. Same MLP shape as the
/// span projections.
pub fn project_prompts(
    weights: &WeightsDebertaV3,
    prompts: &[f32],
    num_prompts: usize,
) -> Vec<f32> {
    run_proj_mlp(&weights.prompt, prompts, num_prompts)
}

/// scores[s * C + c] = span_rep[s] · prompts[c]. `span_rep` is
/// `(num_spans, D)`, `prompts` is `(num_prompts, D)`.
pub fn score(
    span_rep: &[f32],
    prompts: &[f32],
    num_spans: usize,
    num_prompts: usize,
    d: usize,
) -> Vec<f32> {
    let mut out = vec![0.0f32; num_spans * num_prompts];
    for s in 0..num_spans {
        let span_vec = &span_rep[s * d..(s + 1) * d];
        for c in 0..num_prompts {
            let prompt_vec = &prompts[c * d..(c + 1) * d];
            let mut dot = 0.0f32;
            for k in 0..d {
                dot += span_vec[k] * prompt_vec[k];
            }
            out[s * num_prompts + c] = dot;
        }
    }
    out
}

// `PredictedEntity`, `decode`, `generate_span_indices` moved to
// `crate::inference::deberta::decoding` (shared with the INT8 path).
// Re-exported here so existing consumers compile unchanged.
pub use crate::inference::deberta::decoding::{PredictedEntity, decode, generate_span_indices};
