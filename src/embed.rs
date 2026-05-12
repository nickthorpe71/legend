//! Sentence embedding via the bundled all-MiniLM-L6-v2 model.
//!
//! Inference runs through our own pure-Rust BERT engine in
//! `crate::inference` — no `tract-onnx`, no `ort`, no C deps. The
//! tokenizer is HuggingFace's `tokenizers` crate (also pure Rust).
//! All model weights are baked into the binary at compile time.

use std::sync::LazyLock;

use tokenizers::Tokenizer;

use crate::inference::{WeightsInt8, bert_int8};

/// Output dimensionality of the bundled all-MiniLM-L6-v2 model.
/// Single source of truth — callers allocate buffers and arrays against
/// this constant.
pub const EMBEDDING_DIM: usize = 384;

/// True token count of `text` under the bundled MiniLM tokenizer.
/// Used by `lib::run` to gate inputs against `MAX_INPUT_TOKENS`.
/// Uses an *untruncated* tokenizer so the count is accurate for
/// inputs above the model's 512-token max-position limit.
pub fn token_count(text: &str) -> usize {
    UNTRUNCATED_TOKENIZER
        .encode(text, true)
        .expect("tokenization failed")
        .get_ids()
        .len()
}

/// Compute a semantic embedding via the bundled BERT forward pass.
/// Returns a 384-dim L2-normalized vector — cosine similarity reduces
/// to a dot product.
pub fn embed_text(text: &str) -> Vec<f32> {
    if text.trim().is_empty() {
        return vec![0.0f32; EMBEDDING_DIM];
    }
    let encoding = TOKENIZER.encode(text, true).expect("tokenization failed");
    let ids: Vec<u32> = encoding.get_ids().to_vec();
    let mask: Vec<u32> = encoding.get_attention_mask().to_vec();
    let weights: &WeightsInt8 = WeightsInt8::load_bundled();
    bert_int8::forward(weights, &ids, &mask)
}

// Same tokenizer.json as the embedder, but with truncation and
// padding both cleared so `token_count` reports the true length.
// The bundled tokenizer.json ships with `padding: Fixed(128)` baked
// in — without `with_padding(None)` every short input would report
// as 128 tokens.
static UNTRUNCATED_TOKENIZER: LazyLock<Tokenizer> = LazyLock::new(|| {
    let bytes: &[u8] = include_bytes!("../models/all-MiniLM-L6-v2-q/tokenizer.json");
    let mut t = Tokenizer::from_bytes(bytes).expect("Failed to load embedded tokenizer");
    t.with_truncation(None).expect("clear truncation");
    t.with_padding(None);
    t
});

// Embedding-path tokenizer: cap at the model's 512-token max-position
// so a long input doesn't index past the position-embedding table.
// Padding is explicitly disabled — we process one sequence per call
// (no batching) so padding adds purely wasted matmul work. The
// bundled tokenizer.json ships with `padding: Fixed(128)` which would
// silently process a 12-token input as 128 tokens, wasting ~10× of
// the forward-pass cost. `with_padding(None)` strips that.
static TOKENIZER: LazyLock<Tokenizer> = LazyLock::new(|| {
    let bytes: &[u8] = include_bytes!("../models/all-MiniLM-L6-v2-q/tokenizer.json");
    let mut t = Tokenizer::from_bytes(bytes).expect("Failed to load embedded tokenizer");
    let _ = t.with_truncation(Some(tokenizers::TruncationParams {
        max_length: 512,
        ..Default::default()
    }));
    t.with_padding(None);
    t
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embed_empty_returns_zeros() {
        let v = embed_text("");
        assert_eq!(v.len(), EMBEDDING_DIM);
        assert!(v.iter().all(|&x| x == 0.0));
    }

    #[test]
    fn embed_yields_unit_vector() {
        let v = embed_text("hello world");
        assert_eq!(v.len(), EMBEDDING_DIM);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-4,
            "expected unit vector, got norm = {norm}"
        );
    }

    #[test]
    fn embed_is_deterministic() {
        let v1 = embed_text("the meeting is at 3pm");
        let v2 = embed_text("the meeting is at 3pm");
        assert_eq!(v1, v2);
    }

    #[test]
    fn token_count_matches_expected() {
        // "hello world" → [CLS] hello world [SEP] = 4 tokens.
        assert_eq!(token_count("hello world"), 4);
    }
}
