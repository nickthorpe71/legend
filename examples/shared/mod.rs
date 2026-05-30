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
    pub seeded_attribute_names: Vec<RawSeededAttributeName>,
    pub regions: Vec<RawRegion>,
    pub reference_frames: Vec<RawElement>,
    pub units: RawUnits,
    pub seeded_relations: RawSeededRelations,
    pub intent_prototypes: IntentPhrases,
}

/// L2 core-competence "units" block: dimension Elements (Currency,
/// Mass, …) and the unit Elements that pin under them via `instance_of`.
/// Both are plain [`RawNamedElement`]s — `{element_id, names}` — minted
/// as ordinary Signal Elements; the `quantity_arith` kernel reads them
/// to build its conversion `UnitIndex` at load. Conversion factors are
/// NOT in the YAML: they live in the kernel, keyed by surface form.
#[derive(Deserialize)]
pub struct RawUnits {
    pub dimensions: Vec<RawNamedElement>,
    pub members: Vec<RawNamedElement>,
}

/// Anchors and reference frames share this shape: a symbolic ID, one
/// or more surface forms, and a rationale string the seed-graph
/// generator ignores at gen time but the YAML keeps for human readers.
#[derive(Deserialize)]
pub struct RawElement {
    pub element_id: String,
    pub names: Vec<String>,
    pub rationale: String,
}

/// Seeded attribute-name (predicate) entry. Same shape as
/// [`RawElement`] plus an optional `examples` list — short sentences
/// containing the predicate in canonical usage. The seed-graph
/// generator computes the predicate's embedding as the centroid of
/// contextualized span vectors over the examples; when empty, falls
/// back to `embed_text(name)` (the legacy bare-token path).
///
/// Why: predicates need usage centroids, not bare-word vectors, for
/// embedding-knn dedup to work (see `docs/frame-quality-fixes.md`
/// Fix F).
#[derive(Deserialize)]
pub struct RawSeededAttributeName {
    pub element_id: String,
    pub names: Vec<String>,
    pub rationale: String,
    #[serde(default)]
    pub examples: Vec<String>,
}

/// Region entries carry the generator's inputs: `parent_regions`
/// (becomes one `parent_region` relation per parent, with the float as
/// `stats.confidence`), `descriptor` (human-readable category definition,
/// kept for documentation), `examples` (concrete representative
/// sentences that get MiniLM-embedded into per-region prototype elements
/// and drive mean/variance estimation for region-stats routing), and
/// `members` (named child Elements minted under the region — used by
/// closed-class void regions to seed known stop words as graph
/// citizens rather than a hard-coded list).
#[derive(Deserialize)]
pub struct RawRegion {
    pub element_id: String,
    pub names: Vec<String>,
    pub parent_regions: Vec<(String, f32)>,
    pub descriptor: String,
    #[serde(default)]
    pub examples: Vec<String>,
    #[serde(default)]
    pub members: Vec<RawNamedElement>,
    pub rationale: String,
}

/// Named Element attached directly to a region (as opposed to an
/// anonymous prototype derived from an example phrase). Used by the
/// closed-class void regions to seed known stop words.
#[derive(Deserialize)]
pub struct RawNamedElement {
    pub element_id: String,
    pub names: Vec<String>,
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
    pub unit_dimension_pins: RawRelGroup,
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
