//! Relation-extraction patterns. Every place we go from raw text to a
//! candidate relation lives in one of this module's submodules. Three
//! pattern families:
//!
//! - [`ner_anchored`] — surface RE templates over GLiNER spans. Emits
//!   `RelationProposal` (subj/attr/obj all bound by NER spans), used
//!   for event-shaped frames like "X from A to B" / "X at Y".
//! - [`span_typing`] — `(span, instance_of, kind)`. Sources: GLiNER
//!   labels and the temporal regex pass. Emits `ExtractionProposal`.
//! - [`svo`] + [`appositive`] — surface-pattern OpenIE on the
//!   orthographic chunker's output. Model-free. Both emit shared
//!   [`NoveltyRelation`].
//!
//! [`extract_surface_relations`] runs SVO + appositive over the same
//! chunk slice and concatenates their output. Step 5 (run_extractors)
//! orchestrates all three pattern families.
//!
//! Phase 1 of the patterns refactor: types are still split by family
//! (NoveltyRelation / RelationProposal / ExtractionProposal). Phase 2
//! folds them into a single `RelationCandidate`.

pub mod appositive;
pub mod ner_anchored;
pub mod span_typing;
pub mod svo;

pub use appositive::extract_appositives;
pub use ner_anchored::{RelationProposal, extract_relations};
pub use span_typing::{ExtractionProposal, ProposalSource, build_instance_of_proposals};
pub use svo::extract_svo_triples;

use crate::steps::orthographic::OrthographicChunk;

/// Shared output type for both surface-OpenIE patterns (SVO +
/// appositive). Subject and object are char spans into the original
/// input; `attribute_text` is the verbatim connective text to be
/// resolved against attribute-name elements by Step 8.
#[derive(Debug, Clone)]
pub struct NoveltyRelation {
    pub subject_char_start: usize,
    pub subject_char_end: usize,
    /// Trimmed text between the subject and object spans, OR a
    /// canonical attribute name (`"instance_of"` for appositives).
    /// Step 8 resolves via by-name lookup → embedding-knn → mint
    /// Defeasible.
    pub attribute_text: String,
    pub object_char_start: usize,
    pub object_char_end: usize,
    /// Heuristic confidence. Always low — these are candidates for
    /// replay confirmation, not assertions. Step 8 stamps the
    /// resulting Relation `Defeasible`.
    pub confidence: f32,
}

/// Default confidence stamped on every novelty (SVO + appositive)
/// relation. Low enough that any known-branch proposal on the same
/// span outranks it in Step 8's merge.
pub(crate) const DEFAULT_CONFIDENCE: f32 = 0.4;

/// Run both surface-OpenIE pattern families over the same chunk
/// slice. Concatenates SVO + appositive output. The chunk slice must
/// contain both `Phrase`- and `Token`-scale entries (the two
/// sub-extractors filter what they need).
pub fn extract_surface_relations(text: &str, chunks: &[OrthographicChunk]) -> Vec<NoveltyRelation> {
    let mut out = svo::extract_svo_triples(text, chunks);
    out.extend(appositive::extract_appositives(text, chunks));
    out
}
