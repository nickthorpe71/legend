//! Shared types for build-time tools. Imported by examples via `#[path]`
//! since the runtime crate doesn't carry serde — keeping `serde` in
//! `[dev-dependencies]` only.
//!
//! Use from an example like:
//! ```ignore
//! #[path = "shared.rs"]
//! mod shared;
//! use shared::{SeedPack, PhrasePair, dims, load_seed_pack};
//! ```

use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize)]
pub struct SeedPack {
    pub intent_prototypes: IntentPhrases,
}

#[derive(Deserialize)]
pub struct IntentPhrases {
    pub conviction: PhrasePair,
    pub prediction_error: PhrasePair,
    pub arousal: PhrasePair,
    pub curiosity: PhrasePair,
}

#[derive(Deserialize)]
pub struct PhrasePair {
    pub high_pole: Vec<String>,
    pub low_pole: Vec<String>,
    /// Counterfactual pairs — each entry is two sentences sharing topic
    /// but flipped on the pragmatic axis. Drives the build-time
    /// contrastive loss: high should outscore low for every pair, which
    /// forces the classifier to learn intent direction independent of
    /// topical content (Pearl Level-3, controlled experiment).
    pub pairs: Vec<Counterfactual>,
}

#[derive(Deserialize)]
pub struct Counterfactual {
    pub high: String,
    pub low: String,
}

/// Parse `seed_pack.yaml` at the given path. Errors propagate.
pub fn load_seed_pack(path: &Path) -> Result<SeedPack, Box<dyn std::error::Error>> {
    let yaml = std::fs::read_to_string(path)?;
    let pack: SeedPack = serde_yaml::from_str(&yaml)?;
    Ok(pack)
}

/// Iterate the four `(dim_name, pole_pair)` tuples in canonical order.
pub fn dims(pack: &SeedPack) -> [(&'static str, &PhrasePair); 4] {
    [
        ("conviction", &pack.intent_prototypes.conviction),
        ("prediction_error", &pack.intent_prototypes.prediction_error),
        ("arousal", &pack.intent_prototypes.arousal),
        ("curiosity", &pack.intent_prototypes.curiosity),
    ]
}
