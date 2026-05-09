use crate::embed::EMBEDDING_DIM;
use crate::lexical_features::{LEXICAL_FEATURE_COUNT, extract_lexical_features};

/// Total feature count: BGE/MiniLM sentence embedding (384) + hand-crafted
/// lexical features (34) = 418. The classifier dot-products against this
/// concatenated vector at inference.
pub const CLASSIFIER_FEATURE_COUNT: usize = EMBEDDING_DIM + LEXICAL_FEATURE_COUNT;

/// A binary logistic-regression classifier for one intent dimension.
/// Trained at build time by `examples/gen_intent_classifiers.rs` using
/// cross-class negatives — every dim's classifier sees the other three
/// dims' phrases as negatives, which forces the learned direction to
/// point at *what's unique* to this dim's high pole.
///
/// The feature vector concatenates the sentence embedding (which captures
/// topic+intent confounded) with hand-crafted lexical features (which
/// capture intent-causing syntax cleanly — modals, person, mood, etc.).
/// Pearl-flavored: lexical features serve as a front-door mediator,
/// routing around the topic confounding in the embedding.
pub struct Classifier {
    pub weights: [f32; CLASSIFIER_FEATURE_COUNT],
    pub bias: f32,
}

/// Four classifiers, one per intent dimension.
pub struct IntentClassifiers {
    pub conviction: Classifier,
    pub prediction_error: Classifier,
    pub arousal: Classifier,
    pub curiosity: Classifier,
}

// Four classifier blobs baked into the binary at compile time.
// File format: CLASSIFIER_FEATURE_COUNT f32 weights followed by 1 f32 bias,
// all little-endian. Total = (CLASSIFIER_FEATURE_COUNT + 1) * 4 bytes.
const CONVICTION: &[u8] = include_bytes!("intent_classifiers/conviction.bin");
const PREDICTION_ERROR: &[u8] = include_bytes!("intent_classifiers/prediction_error.bin");
const AROUSAL: &[u8] = include_bytes!("intent_classifiers/arousal.bin");
const CURIOSITY: &[u8] = include_bytes!("intent_classifiers/curiosity.bin");

/// Build the input feature vector: sentence embedding (384) ++ hand-crafted
/// lexical features (34). Total `CLASSIFIER_FEATURE_COUNT` (= 418) dims.
/// Single source of truth for the layout — both the build-time trainer and
/// the runtime classifier call this.
///
/// Takes the embedding as input rather than computing it, so the runtime
/// pipeline can compute the embedding once and reuse it across Step 1
/// (intent) and Step 4 (the embedding itself, consumed by Step 5 routing).
/// Build-time trainer call sites compute their own embedding per phrase.
pub fn featurize(text: &str, embedding: &[f32]) -> Vec<f32> {
    debug_assert_eq!(
        embedding.len(),
        EMBEDDING_DIM,
        "embedding must be EMBEDDING_DIM"
    );
    let mut v: Vec<f32> = Vec::with_capacity(CLASSIFIER_FEATURE_COUNT);
    v.extend_from_slice(embedding);
    v.extend_from_slice(&extract_lexical_features(text));
    debug_assert_eq!(v.len(), CLASSIFIER_FEATURE_COUNT);
    v
}

/// Build all four classifiers from the embedded byte slices. Cheap;
/// call once at startup and cache.
pub fn load_intent_classifiers() -> IntentClassifiers {
    IntentClassifiers {
        conviction: parse(CONVICTION),
        prediction_error: parse(PREDICTION_ERROR),
        arousal: parse(AROUSAL),
        curiosity: parse(CURIOSITY),
    }
}

// Parse a tightly-packed [weights | bias] little-endian byte slice.
// Panics if the byte length doesn't match (CLASSIFIER_FEATURE_COUNT + 1) * 4.
fn parse(bytes: &[u8]) -> Classifier {
    assert_eq!(
        bytes.len(),
        (CLASSIFIER_FEATURE_COUNT + 1) * 4,
        "classifier byte slice must be (CLASSIFIER_FEATURE_COUNT + 1) * 4 bytes"
    );
    let mut weights = [0.0f32; CLASSIFIER_FEATURE_COUNT];
    let mut iter = bytes.chunks_exact(4);
    for slot in weights.iter_mut() {
        let chunk = iter.next().expect("weight chunk");
        *slot = f32::from_le_bytes(chunk.try_into().expect("4 bytes"));
    }
    let bias_chunk = iter.next().expect("bias chunk");
    let bias = f32::from_le_bytes(bias_chunk.try_into().expect("4 bytes"));
    Classifier { weights, bias }
}
