//! End-to-end GLiNER2 prediction: raw text + label list → labeled
//! character-offset spans. Wraps the preprocessor + INT8 forward pass
//! + decoder + word→char mapping.
//!
//! Public entry point for Step 5 (`run_extractors`).

use crate::inference::deberta::forward_int8::predict_entities_int8;
use crate::inference::deberta::preprocess::{build_inputs, Word};
use crate::inference::deberta::weights_int8::WeightsDebertaInt8;

/// One labeled entity, with character offsets ready to feed into the
/// Step 5 proposal queue.
#[derive(Debug, Clone, PartialEq)]
pub struct LabeledSpan {
    /// Inclusive char offset of the first character.
    pub char_start: usize,
    /// Exclusive char offset (one past last char).
    pub char_end: usize,
    pub label: String,
    pub text: String,
    pub score: f32,
}

/// Convenience: run the full pipeline against the bundled INT8 weights.
pub fn predict_entities(text: &str, labels: &[&str], threshold: f32) -> Vec<LabeledSpan> {
    let weights = WeightsDebertaInt8::load_bundled();
    predict_entities_with_weights(weights, text, labels, threshold)
}

/// Same as `predict_entities` but with an explicit weight handle —
/// useful when callers want to share `load_bundled()` across many
/// inferences without going through the lazy static each time.
pub fn predict_entities_with_weights(
    weights: &WeightsDebertaInt8,
    text: &str,
    labels: &[&str],
    threshold: f32,
) -> Vec<LabeledSpan> {
    let inputs = build_inputs(text, labels);
    let raw = predict_entities_int8(
        weights,
        &inputs.input_ids,
        &inputs.attention_mask,
        &inputs.words_mask,
        weights.max_width,
        threshold,
    );

    raw.entities
        .into_iter()
        .filter_map(|e| {
            // Defensive: a predicted span must reference actual words.
            // Out-of-range indices indicate a bug upstream — drop and
            // keep the rest of the proposals.
            let start = inputs.words.get(e.word_start)?;
            let end = inputs.words.get(e.word_end)?;
            let label = labels.get(e.label_idx)?;
            Some(LabeledSpan {
                char_start: start.char_start,
                char_end: end.char_end,
                label: (*label).to_string(),
                text: text[start.char_start..end.char_end].to_string(),
                score: e.score,
            })
        })
        .collect()
}

/// Re-exported so callers building inputs by hand can access the same
/// `Word` struct returned by `build_inputs`.
pub use crate::inference::deberta::preprocess::build_inputs as build_gliner_inputs;
pub type GlinerWord = Word;
