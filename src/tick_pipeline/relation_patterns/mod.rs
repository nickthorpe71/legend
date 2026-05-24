//! Relation-extraction patterns. Every place we go from raw text to a
//! candidate relation lives in one of this module's submodules and
//! emits the shared [`RelationCandidate`] type — so Step 8 has a
//! single consumer path regardless of pattern source.
//!
//! Pattern families:
//!
//! - [`ner_anchored`] — surface RE templates over GLiNER spans.
//!   Event-shaped frames like "X from A to B", "X at Y", "X's Y".
//! - [`span_typing`] — `(span, instance_of, kind)`. Sources: GLiNER
//!   labels and the temporal regex pass.
//! - [`svo`] + [`appositive`] — surface-pattern OpenIE on the
//!   orthographic chunker's output. Model-free; always emits
//!   `Defeasible` candidates.
//!
//! Step 5 (`run_extractors`) is responsible for calling each family
//! and partitioning the results into three buckets:
//!
//! - **instance_of** — `ObjectRef::Label`-bearing candidates (span
//!   typing). Step 8 resolves the label via the seed-pack lookup.
//! - **relations** — NER-anchored candidates with `Span` objects.
//!   Step 8 runs n-ary event-merge over these first.
//! - **surface** — SVO + appositive candidates. Always Defeasible;
//!   merged after the known-branch proposals.

pub mod appositive;
pub mod ner_anchored;
pub mod span_typing;
pub mod svo;

pub use appositive::extract_appositives;
pub use ner_anchored::extract_relations;
pub use span_typing::build_instance_of_proposals;
pub use svo::extract_svo_triples;

use crate::tick_pipeline::orthographic::OrthographicChunk;
use crate::types::RelationStatus;

/// One candidate relation emitted by any pattern family. Step 8
/// consumes a homogeneous slice of these and dispatches on
/// [`ObjectRef`] (span vs label) and [`event_anchor`] (n-ary event
/// merge) rather than on pattern source.
///
/// [`event_anchor`]: RelationCandidate::event_anchor
#[derive(Debug, Clone)]
pub struct RelationCandidate {
    /// Which pattern family produced this candidate. Behavioral
    /// decisions in Step 8 are driven by the data fields below, not
    /// by source — `source` is for telemetry and dedup heuristics.
    pub source: PatternSource,

    pub subject_char_start: usize,
    pub subject_char_end: usize,

    /// Attribute element name. Step 8 resolves via:
    /// (1) `hg.by_name` lookup, (2) typed-relation lexicon, (3)
    /// embedding-knn over attribute-name centroids, (4) mint a
    /// Defeasible attribute element if all three miss. Canonical
    /// seed-attribute names ("from", "to", "at", "instance_of")
    /// hit (1).
    pub attribute_name: String,

    /// Character range of the predicate's surface form in the source
    /// input. SVO populates this; other emitters that use canonical
    /// predicate names (NER-anchored templates, span-typing,
    /// appositive) leave it `None` because their predicate is a
    /// `&'static str` not present at a specific char range.
    /// Step 8 uses it to compute the predicate's contextualized
    /// span embedding when resolving novel attribute names.
    pub attribute_char_start: Option<usize>,
    pub attribute_char_end: Option<usize>,

    /// Object reference. `Span` for objects that are sub-spans of
    /// `input_text` (Step 8 slices + resolves by name); `Label` for
    /// objects that are known class/kind elements addressed by name
    /// (Step 8 calls `resolve_label_element`).
    pub object: ObjectRef,

    pub confidence: f32,
    pub status: RelationStatus,

    /// Verb anchor for n-ary event merging — pairs `from`/`to`
    /// candidates into a single event relation. `None` for non-event
    /// patterns.
    pub event_anchor: Option<String>,
}

/// How the object should be resolved to an `ElementId` at mint time.
#[derive(Debug, Clone)]
pub enum ObjectRef {
    /// Object is a sub-span of `input_text`. Step 8 slices, looks up
    /// by name, mints a Defeasible element on miss.
    Span { char_start: usize, char_end: usize },
    /// Object is a known class/kind element addressed by name
    /// (e.g. `"weekday"`, `"month"`, `"person"`, `"unknown_prior"`).
    /// Step 8 resolves via the seed-pack label table.
    Label(String),
}

/// Which pattern family emitted a candidate. Used for telemetry and
/// for any source-specific dedup heuristic; never branched on by the
/// substrate-mutating code in Step 8.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternSource {
    /// NER-anchored RE templates ("X from A to B", "X at Y", "X's Y").
    NerAnchored,
    /// Verb-shape SVO triples from content tokens.
    Svo,
    /// Comma-appositive — emits `instance_of`.
    Appositive,
    /// GLiNER label → `(span, instance_of, label)`.
    NerSpanTyping,
    /// Temporal regex → `(span, instance_of, weekday|month|time)`.
    TemporalSpanTyping,
}

/// Default confidence stamped on every surface (SVO + appositive)
/// candidate. Low enough that any known-branch candidate on the same
/// span outranks it in Step 8's merge.
pub(crate) const DEFAULT_SURFACE_CONFIDENCE: f32 = 0.4;

/// Run both surface-OpenIE pattern families over the same chunk
/// slice. Concatenates SVO + appositive output. The chunk slice must
/// contain both `Phrase`- and `Token`-scale entries.
pub fn extract_surface_relations(
    text: &str,
    chunks: &[OrthographicChunk],
) -> Vec<RelationCandidate> {
    let mut out = svo::extract_svo_triples(text, chunks);
    out.extend(appositive::extract_appositives(text, chunks));
    out
}
