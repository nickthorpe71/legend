use std::sync::LazyLock;

use crate::intent_classifiers::{
    Classifier, IntentClassifiers, featurize, load_intent_classifiers,
};
use crate::math::{dot, sigmoid};
use crate::types::Intent;

// Load the four binary classifiers once, on first use, then keep them in
// memory for the rest of the process. ~7 KB resident; negligible.
static CLASSIFIERS: LazyLock<IntentClassifiers> = LazyLock::new(load_intent_classifiers);

/// Step 1 of the tick pipeline. Build a feature vector from the input
/// (sentence embedding ++ hand-crafted lexical features), run it through
/// four binary logistic-regression classifiers (one per intent dimension),
/// and return a 4-dim intent vector with each component in [0, 1].
///
/// Per-dim classifiers are trained at build time with cross-class
/// negatives (other dims' phrases as additional negatives), so the learned
/// directions are roughly orthogonal across dimensions — Pearl-flavored
/// confounder adjustment.
///
/// Lexical features (modal counts, person, mood, punctuation, tense)
/// serve as a front-door mediator capturing intent-causing syntax cleanly,
/// without the topic confounding the embedding carries.
///
/// Takes the embedding precomputed by the caller — the same vector also
/// feeds Step 4 (region routing). One embedding per tick, never two.
pub fn detect_intent(input_text: &str, embedding: &[f32]) -> Intent {
    let features = featurize(input_text, embedding);
    let c = &*CLASSIFIERS;

    Intent {
        conviction: score(&features, &c.conviction),
        prediction_error: score(&features, &c.prediction_error),
        arousal: score(&features, &c.arousal),
        curiosity: score(&features, &c.curiosity),
    }
}

fn score(features: &[f32], clf: &Classifier) -> f32 {
    sigmoid(clf.bias + dot(features, &clf.weights))
}
