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
//!
//! The module-level `#![allow(dead_code)]` is intentional. Each example
//! that does `#[path] mod shared;` compiles the entire file and reads
//! only the slice it needs (intent classifier examples never touch
//! `regions`; the seed-graph generator never touches
//! `intent_prototypes`). Per-field `dead_code` warnings would fire on
//! every example build despite the fields being load-bearing for
//! `serde::Deserialize` — the field names drive YAML matching, not
//! Rust read sites.

#![allow(dead_code)]

use serde::Deserialize;
use std::path::Path;

/// Top-level shape of `seed_pack.yaml`. Built-time tools (intent
/// classifier trainer, seed-graph generator) deserialize the whole pack
/// and consume their own slice — `gen_intent_classifiers` reads
/// `intent_prototypes`, `gen_seed_graph` reads everything else.
#[derive(Deserialize)]
pub struct SeedPack {
    pub anchors: Vec<RawElement>,
    pub seeded_attribute_names: Vec<RawElement>,
    pub regions: Vec<RawRegion>,
    pub reference_frames: Vec<RawElement>,
    pub seeded_relations: RawSeededRelations,
    pub intent_prototypes: IntentPhrases,
}

/// Anchors, seeded attribute names, and reference frames all share this
/// shape: a symbolic ID, one or more surface forms, and a rationale
/// string the seed-graph generator ignores at gen time but the YAML
/// keeps for human readers.
#[derive(Deserialize)]
pub struct RawElement {
    pub element_id: String,
    pub names: Vec<String>,
    pub rationale: String,
}

/// Region entries carry two extras the generator needs: `parent_regions`
/// (becomes one `parent_region` relation per parent, with the float as
/// `stats.confidence`) and `descriptor` (gets MiniLM-embedded to
/// produce the region's day-zero prototype Element).
#[derive(Deserialize)]
pub struct RawRegion {
    pub element_id: String,
    pub names: Vec<String>,
    pub parent_regions: Vec<(String, f32)>,
    pub descriptor: String,
    pub rationale: String,
}

/// Three blocks of structural seed relations. Each block's `relations`
/// list is heterogeneous — most entries are `[A, attr, B]` (3-element)
/// but `region_parent_pins` adds a 4th entry for the parent-edge
/// confidence weight. Deserialized as raw YAML values; the generator
/// unpacks each entry positionally.
#[derive(Deserialize)]
pub struct RawSeededRelations {
    pub region_class_pins: RawRelGroup,
    pub reference_frame_class_pins: RawRelGroup,
    pub region_parent_pins: RawRelGroup,
}

#[derive(Deserialize)]
pub struct RawRelGroup {
    pub rationale: String,
    pub relations: Vec<Vec<serde_yaml::Value>>,
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
