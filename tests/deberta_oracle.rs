//! End-to-end validation of the pure-Rust GLiNER2 forward pass
//! against the PyTorch oracle captured by
//! `oracle/02_capture_oracle.py`. Tests escalate from
//! per-stage outputs (embedding, layer 0) up through the full encoder
//! stack — when one fails, the lowest-numbered failure tells you which
//! piece broke.
//!
//! All tests are gated on the `gliner2_fp32` feature: they need the
//! 582 MB bundled weights to run.

#![cfg(feature = "gliner2_fp32")]

use std::fs;
use std::path::{Path, PathBuf};

use legend::inference::deberta::embedding::embed_and_layernorm;
use legend::inference::deberta::encoder::{run_encoder_stack, run_layer};
use legend::inference::deberta::head::{
    build_span_rep, decode, generate_span_indices, project_prompts, project_tokens,
    run_bilstm, score, split_tokens,
};
use legend::inference::deberta::rel_pos::build_relative_position_matrix;
use legend::inference::deberta::weights::WeightsDebertaV3;
use legend::inference::ops::layernorm_inplace;

// Fixture: oracle/fixtures/dentist/.
// 24 tokens, single sentence "My dentist appointment with Dr. Rao …"
// Captured input_ids reproduced here verbatim from
// `oracle/fixtures/dentist/tokenizer.json`.
const DENTIST_IDS: &[u32] = &[
    1, 128002, 604, 128002, 720, 128002, 20467, 128002, 985, 128003, 573, 8301, 3198, 275, 1011,
    323, 25773, 1594, 292, 1586, 264, 1178, 323, 2,
];
const DENTIST_MASK: &[u32] = &[1; 24];
const DENTIST_WORDS_MASK: &[u32] = &[
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 0,
];
const DENTIST_NUM_WORDS: usize = 13;
const DENTIST_NUM_PROMPTS: usize = 4;
const DENTIST_LABELS: &[&str] = &["person", "event", "weekday", "role"];
const DENTIST_MAX_WIDTH: usize = 12;
const DENTIST_THRESHOLD: f32 = 0.3;

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("oracle")
        .join("fixtures")
        .join("dentist")
        .join(name)
}

fn read_f32_bin(name: &str) -> Vec<f32> {
    let path = fixture_path(name);
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    assert!(
        bytes.len().is_multiple_of(4),
        "fixture {name} not a multiple of 4 bytes"
    );
    let n = bytes.len() / 4;
    let mut out = vec![0.0f32; n];
    let dst =
        unsafe { std::slice::from_raw_parts_mut(out.as_mut_ptr() as *mut u8, bytes.len()) };
    dst.copy_from_slice(&bytes);
    out
}

/// Element-wise comparison metrics. We require both a tight max-abs
/// bound and high cosine similarity so a stray NaN or a single bad
/// dimension can't slip through.
fn assert_tensor_matches(label: &str, got: &[f32], expected: &[f32], max_abs: f32, min_cos: f32) {
    assert_eq!(
        got.len(),
        expected.len(),
        "{label}: length mismatch (got {}, expected {})",
        got.len(),
        expected.len()
    );

    let mut max_diff = 0.0f32;
    let mut max_diff_idx = 0usize;
    let mut dot = 0.0f64;
    let mut na = 0.0f64;
    let mut nb = 0.0f64;
    for (i, (&g, &e)) in got.iter().zip(expected.iter()).enumerate() {
        assert!(g.is_finite(), "{label}: NaN/inf at index {i}: {g}");
        let d = (g - e).abs();
        if d > max_diff {
            max_diff = d;
            max_diff_idx = i;
        }
        dot += g as f64 * e as f64;
        na += g as f64 * g as f64;
        nb += e as f64 * e as f64;
    }
    let cos = dot / (na.sqrt() * nb.sqrt()).max(1e-30);

    println!(
        "{label}: max_abs={max_diff:.6e} (at {max_diff_idx}), cos={cos:.6} | tol max_abs<{max_abs:.1e}, cos>{min_cos}"
    );
    assert!(
        max_diff < max_abs,
        "{label}: max abs diff {max_diff:.6e} exceeds tol {max_abs:.1e} at index {max_diff_idx} (got={}, expected={})",
        got[max_diff_idx],
        expected[max_diff_idx]
    );
    assert!(
        cos > min_cos as f64,
        "{label}: cosine {cos:.6} below tol {min_cos}"
    );
}

#[test]
fn embedding_matches_oracle() {
    let w = WeightsDebertaV3::load_bundled();
    let got = embed_and_layernorm(w, DENTIST_IDS, DENTIST_MASK);
    let expected = read_f32_bin("embedding.bin");
    assert_tensor_matches("embedding", &got, &expected, 1e-4, 0.99999);
}

#[test]
fn layer_0_matches_oracle() {
    let w = WeightsDebertaV3::load_bundled();
    let seq_len = DENTIST_IDS.len();
    let hidden = w.hidden_size;
    let rel_table_len = 2 * w.position_buckets;

    let mut x = embed_and_layernorm(w, DENTIST_IDS, DENTIST_MASK);

    // Pre-compute rel_pos + LN'd rel_emb the same way run_encoder_stack does.
    let rel_pos_index =
        build_relative_position_matrix(seq_len, w.position_buckets, w.max_position);
    let mut rel_emb_lnd = w.rel_emb.clone();
    layernorm_inplace(
        &mut rel_emb_lnd,
        rel_table_len,
        hidden,
        &w.rel_emb_ln_gamma,
        &w.rel_emb_ln_beta,
        w.layer_norm_eps,
    );

    x = run_layer(
        x,
        &w.layers[0],
        &rel_emb_lnd,
        &rel_pos_index,
        DENTIST_MASK,
        seq_len,
        hidden,
        w.num_heads,
        w.head_dim,
        w.intermediate_size,
        rel_table_len,
        w.layer_norm_eps,
    );

    let expected = read_f32_bin("layer_0.bin");
    assert_tensor_matches("layer_0", &x, &expected, 5e-3, 0.9999);
}

#[test]
fn encoder_stack_matches_oracle() {
    let w = WeightsDebertaV3::load_bundled();
    let x = embed_and_layernorm(w, DENTIST_IDS, DENTIST_MASK);
    let out = run_encoder_stack(w, x, DENTIST_MASK);
    let expected = read_f32_bin("encoder_out.bin");
    // Looser tolerance after 6 layers of accumulated rounding.
    assert_tensor_matches("encoder_out", &out, &expected, 5e-2, 0.999);
}

// --- Head pipeline -------------------------------------------------

fn run_through_encoder() -> Vec<f32> {
    let w = WeightsDebertaV3::load_bundled();
    let x = embed_and_layernorm(w, DENTIST_IDS, DENTIST_MASK);
    run_encoder_stack(w, x, DENTIST_MASK)
}

#[test]
fn projection_matches_oracle() {
    let w = WeightsDebertaV3::load_bundled();
    let enc = run_through_encoder();
    let got = project_tokens(w, &enc, DENTIST_IDS.len());
    let expected = read_f32_bin("projection.bin");
    assert_tensor_matches("projection", &got, &expected, 5e-2, 0.999);
}

#[test]
fn split_matches_oracle() {
    let w = WeightsDebertaV3::load_bundled();
    let enc = run_through_encoder();
    let projected = project_tokens(w, &enc, DENTIST_IDS.len());
    let split = split_tokens(
        w,
        &projected,
        DENTIST_IDS,
        DENTIST_WORDS_MASK,
        DENTIST_IDS.len(),
    );
    assert_eq!(split.num_words, DENTIST_NUM_WORDS);
    assert_eq!(split.num_prompts, DENTIST_NUM_PROMPTS);
    let expected_words = read_f32_bin("words.bin");
    let expected_prompts = read_f32_bin("prompts.bin");
    assert_tensor_matches("words", &split.words, &expected_words, 5e-2, 0.999);
    assert_tensor_matches("prompts", &split.prompts, &expected_prompts, 5e-2, 0.999);
}

#[test]
fn lstm_matches_oracle() {
    // Drive the LSTM straight from the oracle's `words.bin` so the
    // test isolates LSTM correctness from upstream encoder drift.
    let w = WeightsDebertaV3::load_bundled();
    let words = read_f32_bin("words.bin");
    let out = run_bilstm(w, &words, DENTIST_NUM_WORDS);
    let expected = read_f32_bin("lstm_out.bin");
    assert_tensor_matches("lstm_out", &out, &expected, 5e-3, 0.9999);
}

#[test]
fn span_rep_matches_oracle() {
    let w = WeightsDebertaV3::load_bundled();
    // Span representation is computed from `lstm_out` (the words after
    // BiLSTM). Use the oracle's lstm_out to remove upstream drift.
    let lstm_out = read_f32_bin("lstm_out.bin");
    let (spans, _valid) = generate_span_indices(DENTIST_NUM_WORDS, DENTIST_MAX_WIDTH);
    assert_eq!(spans.len(), DENTIST_NUM_WORDS * DENTIST_MAX_WIDTH);
    let got = build_span_rep(w, &lstm_out, DENTIST_NUM_WORDS, &spans);
    let expected = read_f32_bin("span_rep.bin");
    // The oracle saves span_rep as (1, W, K, D) flat; ours is (W*K, D)
    // flat. Same layout because (W, K, D) reshape preserves row order.
    assert_tensor_matches("span_rep", &got, &expected, 5e-2, 0.999);
}

#[test]
fn prompts_final_matches_oracle() {
    let w = WeightsDebertaV3::load_bundled();
    let prompts = read_f32_bin("prompts.bin");
    let got = project_prompts(w, &prompts, DENTIST_NUM_PROMPTS);
    let expected = read_f32_bin("prompts_final.bin");
    assert_tensor_matches("prompts_final", &got, &expected, 5e-2, 0.999);
}

#[test]
fn scores_match_oracle() {
    let w = WeightsDebertaV3::load_bundled();
    let span_rep = read_f32_bin("span_rep.bin");
    let prompts_final = read_f32_bin("prompts_final.bin");
    let d = w.projection_out;
    let n_spans = span_rep.len() / d;
    let got = score(&span_rep, &prompts_final, n_spans, DENTIST_NUM_PROMPTS, d);
    let expected = read_f32_bin("scores.bin");
    // Scores are pre-sigmoid logits — bigger magnitudes; allow a bit more.
    assert_tensor_matches("scores", &got, &expected, 5e-2, 0.9999);
}

#[test]
fn decode_matches_oracle_entities() {
    // End-to-end through our pure-Rust pipeline. The decoded entities
    // (label + word boundaries) must match the oracle's
    // `entities.json` exactly (modulo char-position mapping, which is
    // the caller's job).
    //
    // Oracle entities for dentist:
    //   [ 0:22] event       'My dentist appointment'   word [0, 2]
    //   [28:35] person      'Dr. Rao'                  word [4, 6]
    //   [49:56] weekday     'Tuesday'                  word [9, 9]
    //   [60:66] weekday     'Friday'                   word [11, 11]
    let w = WeightsDebertaV3::load_bundled();

    // Run the whole pipeline.
    let x = embed_and_layernorm(w, DENTIST_IDS, DENTIST_MASK);
    let enc = run_encoder_stack(w, x, DENTIST_MASK);
    let projected = project_tokens(w, &enc, DENTIST_IDS.len());
    let split = split_tokens(w, &projected, DENTIST_IDS, DENTIST_WORDS_MASK, DENTIST_IDS.len());
    let lstm = run_bilstm(w, &split.words, split.num_words);
    let (spans, valid) = generate_span_indices(split.num_words, DENTIST_MAX_WIDTH);
    let span_rep = build_span_rep(w, &lstm, split.num_words, &spans);
    let prompts = project_prompts(w, &split.prompts, split.num_prompts);
    let scores = score(&span_rep, &prompts, spans.len(), split.num_prompts, w.projection_out);
    let entities = decode(&scores, &spans, &valid, split.num_prompts, DENTIST_THRESHOLD);

    println!("decoded {} entities:", entities.len());
    for e in &entities {
        println!(
            "  word [{}:{}]  {:<10}  ({:.3})",
            e.word_start, e.word_end, DENTIST_LABELS[e.label_idx], e.score
        );
    }

    // Hard expectations.
    assert_eq!(entities.len(), 4, "expected 4 entities, got {}", entities.len());
    let by_pos: Vec<(usize, usize, &str)> = entities
        .iter()
        .map(|e| (e.word_start, e.word_end, DENTIST_LABELS[e.label_idx]))
        .collect();
    assert_eq!(by_pos[0], (0, 2, "event"));
    assert_eq!(by_pos[1], (4, 6, "person"));
    assert_eq!(by_pos[2], (9, 9, "weekday"));
    assert_eq!(by_pos[3], (11, 11, "weekday"));
}
