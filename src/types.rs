use std::collections::HashMap;

// ─────────────────────────────────────────────────────────────────────────────
// Identity & time
// ─────────────────────────────────────────────────────────────────────────────

/// Monotonic logical clock for the Hypergraph. Bumped once per `tick()` call.
/// Used as a timestamp on every mint (`created_at`), every access
/// (`last_seen`, `last_accessed`), and as the unit for decay / promotion
/// windows (e.g. `promotion_window_ticks`). Not wall-clock time —
/// real-world timestamps live on `Input.wall_clock`.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
pub struct Tick(pub u64);

/// Local handle into `Hypergraph.elements`. Newtype around `u32` so the
/// compiler refuses to mix it with `RelationId` or any other index. Not
/// globally unique — when cross-store sync arrives, a separate stable
/// identifier (e.g. `Ulid`) will sit alongside this on each Element.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct ElementId(pub u32);

/// Local handle into `Hypergraph.relations`. Same pattern as `ElementId`.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct RelationId(pub u32);

// ─────────────────────────────────────────────────────────────────────────────
// Hypergraph — Legend's model of the world
// ─────────────────────────────────────────────────────────────────────────────

/// The world model. Elements are bare identities; Relations are claims
/// between them. The hot path reads from this alone; the WAL is for
/// crash recovery between checkpoints.
#[derive(Debug)]
pub struct Hypergraph {
    /// Concrete entities — concepts, surface forms, attribute-name elements,
    /// region anchors, typed leaf values ("Tuesday", "6 pounds", "Berlin").
    /// Each carries an inline embedding so similarity lookups don't need a
    /// side table.
    pub elements: Vec<Element>,

    /// Claims/edges between elements. A relation is a flat list of named
    /// attributes — meta-relations (frame, source, supersession, region
    /// topology) are ordinary relations whose attribute list contains a
    /// `Term::Relation(...)` value pointing at the parent.
    pub relations: Vec<Relation>,

    /// Logical now. Read at the start of each tick, embedded into every
    /// `created_at` / `last_seen` field, then bumped before the next call.
    pub clock: Tick,

    /// Tuning constants every pipeline step reads. Step 2 produces a
    /// per-tick adjusted Policy from this rest-state value; Steps 4–12
    /// see the adjusted view. Only PFC writes the rest-state Policy.
    pub policy: Policy,

    /// Working-memory ring buffer of recent focal points. Step 4 inherits
    /// `active_frame` from here when the input's frame is unset and not
    /// shifting. Capacity is bounded by `policy.recent_focus_capacity`;
    /// oldest entries fall off the back. `VecDeque` (not `Vec`) for O(1)
    /// push-front / pop-back behavior.
    pub recent_focus: std::collections::VecDeque<RecentFocusEntry>,

    // ── Derived indices ────────────────────────────────────────────────
    //
    // Denormalized read caches over `relations` and `elements`. They are
    // *never* serialized into the seed bin or any future snapshot —
    // `seed::rebuild_indices(&mut hg)` rebuilds them after deserialization
    // and after any test that hand-builds the graph. Keeping them out of
    // the on-disk format means the relation graph is the single source
    // of truth and we can change index shapes without versioning the bin.
    /// Surface-form lookup — every element name maps to its `ElementId`s.
    /// Drives attribute-name resolution at extractor mint time, name-based
    /// boot lookups, and debug dumps. `Vec<ElementId>` per name because
    /// the canonical-name space is shared across elements (e.g. `"object"`
    /// legitimately resolves to both a coding-region element and a
    /// philosophy-region element — disambiguation happens via region
    /// routing, not here).
    pub by_name: HashMap<String, Vec<ElementId>>,

    /// DAG-descent topology read by Step 4 region routing. For each
    /// region `R`, the list of its child regions (regions whose own
    /// `parent_region` attribute points at `R`). Derived from
    /// `parent_region` relations on every load; the relation graph is
    /// authoritative.
    pub region_children: HashMap<ElementId, Vec<ElementId>>,

    /// Inverse of `region_children` — for each region `R`, the list of
    /// its parents along with the parent-edge confidence weight (read
    /// from each `parent_region` relation's `stats.confidence`).
    /// Multi-parent attachment is allowed (§10.2); regions can sit
    /// under multiple parents with different weights.
    pub region_parents: HashMap<ElementId, Vec<(ElementId, f32)>>,

    /// For each region `R`, the prototype Elements attached via
    /// `(R, prototype, P)` relations. Read by Step 4 routing — at each
    /// candidate region, the score is `max cosine` over each prototype's
    /// inline `embedding`. v0 day zero ships one prototype per seeded
    /// region (built from the YAML `descriptor`); replay grows or merges
    /// the set per §10.4 / §14.8.
    pub region_prototypes: HashMap<ElementId, Vec<ElementId>>,

    /// For each region `R`, the per-dimension mean + variance of its
    /// prototype elements' embeddings. Computed from
    /// `region_prototypes[R]` during `rebuild_indices`; lets Step 4
    /// score input via diagonal Mahalanobis distance instead of
    /// max-cosine when many-prototype data is available. With a single
    /// prototype `var` collapses to all-zeros — Step 4 mixes a
    /// variance prior at use time to keep the score well-defined.
    pub region_stats: HashMap<ElementId, RegionStats>,

    // ── Step 8 relation indices ────────────────────────────────────────
    //
    // Populated by `rebuild_indices` over `hg.relations` and updated
    // incrementally by Step 8 (`build_relations`) per minted relation.
    // None of these are serialized — they're derived caches that
    // rebuild deterministically from the relations themselves.
    /// For each Element `E`, the list of Relations that mention `E` as
    /// the value of any of their attributes. Drives "what does the
    /// graph claim about E?" lookups (Step 9's supersession search,
    /// Step 12's focus walk).
    pub relations_by_element: HashMap<ElementId, Vec<RelationId>>,

    /// For each attribute-name Element `A`, the list of Relations that
    /// use `A` as one of their attribute names. Drives label-set
    /// resolution and Step 9's filtering ("relations with a `property`
    /// attribute").
    pub relations_by_attribute_name: HashMap<ElementId, Vec<RelationId>>,

    /// For each Relation `R`, the list of meta-relations whose
    /// `target` attribute points at `R`. Chain walks (supersession,
    /// derivation) go through here.
    pub meta_relations_by_subject: HashMap<RelationId, Vec<RelationId>>,

    /// For each Relation `R`, the list of meta-relations whose
    /// non-`target` attribute slot references `R` via `Term::Relation`.
    /// The inverse of `meta_relations_by_subject` — lets a relation
    /// find which meta-relations name it as their object.
    pub meta_relations_by_object: HashMap<RelationId, Vec<RelationId>>,

    /// Per `(attribute_name, value)` pair, the count of base relations
    /// that bind that pair. Cheap recognition signal: "how many
    /// relations say X is an instance_of person?" without scanning
    /// `relations`.
    pub attribute_value_counts: HashMap<(ElementId, ElementId), u32>,

    /// Per ordered pair of attribute names co-occurring on the same
    /// relation's attribute list, the count of relations exhibiting
    /// that co-occurrence. Drives §3.4 frame recognition.
    pub attribute_co_counts: HashMap<(ElementId, ElementId), u32>,

    /// Per `(relation, attribute_name)`, presence flag: does relation
    /// `R` carry a meta-attribute with this name? `true` iff some
    /// meta-relation `[target: R, ..., <attribute_name>: ...]` exists.
    /// Populated by walking every relation's attributes — when an
    /// attribute is `Term::Relation(R)` and named `target`, every
    /// non-`target` sibling attribute on that relation gets marked
    /// present on `R`. Lets Step 9 / Step 10 ask
    /// "does this event have an `intervened` meta?" in O(1).
    pub meta_relation_presence: HashMap<(RelationId, ElementId), bool>,

    // ── Anchor IDs ─────────────────────────────────────────────────────
    //
    // Cached at seed-load time so hot-path lookups don't have to round-
    // trip through `by_name`. A `Default`-constructed Hypergraph fills
    // these with `ElementId(u32::MAX)` sentinels — useful for tests but
    // unusable in production (any lookup against a sentinel ID will
    // panic on the elements bounds check).
    /// Sink for retracted / pruned elements that must remain referenceable
    /// without participating in routing or focus. Step 4 routes
    /// sub-threshold inputs here. Seeded as `ElementId(0)` at gen time.
    pub void: ElementId,

    /// Root of the region DAG — every seeded region declares Genesis as
    /// a parent, so Step 4 routing always has a single starting point.
    /// Seeded as `ElementId(1)` at gen time.
    pub genesis: ElementId,

    /// The class element targeted by every seeded region's
    /// `(R, instance_of, REGION_CLASS)` pin. §3.4's
    /// concept-of-many-instances recognition reads this; eagerly minted
    /// by the seed-pack generator (the YAML treats it as lazy, but a
    /// stable boot-time ID is cheaper than minting on first reference).
    pub region_class: ElementId,

    /// Same role as `region_class` for reference frames — every
    /// `(F, instance_of, REFERENCE_FRAME_CLASS)` pin points here.
    /// Eagerly minted at gen time.
    pub reference_frame_class: ElementId,

    /// Cached ID of the seeded `subject` attribute name. Stored as a
    /// field so `rebuild_indices` and downstream extractors don't
    /// round-trip through `by_name["subject"]` on the hot path.
    pub subject_attr: ElementId,

    /// Cached ID of the seeded `parent_region` attribute name. Used by
    /// `rebuild_indices` to know which relations to project into
    /// `region_children` / `region_parents`.
    pub parent_region_attr: ElementId,

    /// Cached ID of the seeded `prototype` attribute name. Same role
    /// as `parent_region_attr`, but for the prototype index.
    pub prototype_attr: ElementId,

    /// Cached ID of the seeded `target` attribute name. Meta-relations
    /// use `target` to point at their parent relation
    /// (`(target: R_parent, derived_from: ...)`); the index splits on
    /// this attribute name to populate `meta_relations_by_subject`
    /// vs `meta_relations_by_object`.
    pub target_attr: ElementId,
}

impl Default for Hypergraph {
    /// Empty Hypergraph with sentinel anchor IDs (`ElementId(u32::MAX)`).
    /// Useful for tests that hand-build state. Production callers should
    /// use `seed::load_seed_graph()` — sentinel IDs will fail any real
    /// element/relation lookup.
    fn default() -> Self {
        Self {
            elements: Vec::new(),
            relations: Vec::new(),
            clock: Tick::default(),
            policy: Policy::default(),
            recent_focus: std::collections::VecDeque::new(),
            by_name: HashMap::new(),
            region_children: HashMap::new(),
            region_parents: HashMap::new(),
            region_prototypes: HashMap::new(),
            region_stats: HashMap::new(),
            relations_by_element: HashMap::new(),
            relations_by_attribute_name: HashMap::new(),
            meta_relations_by_subject: HashMap::new(),
            meta_relations_by_object: HashMap::new(),
            attribute_value_counts: HashMap::new(),
            attribute_co_counts: HashMap::new(),
            meta_relation_presence: HashMap::new(),
            void: ElementId(u32::MAX),
            genesis: ElementId(u32::MAX),
            region_class: ElementId(u32::MAX),
            reference_frame_class: ElementId(u32::MAX),
            subject_attr: ElementId(u32::MAX),
            parent_region_attr: ElementId(u32::MAX),
            prototype_attr: ElementId(u32::MAX),
            target_attr: ElementId(u32::MAX),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Substrate types — what the Hypergraph is made of
// ─────────────────────────────────────────────────────────────────────────────

/// A bare identity in the world. Region topology, typed values, and
/// attribute names are all just Elements — discrimination happens via
/// the relations that mention them, not via a type tag on this struct.
#[derive(Clone, Debug)]
pub struct Element {
    /// Stable handle within this Hypergraph.
    pub id: ElementId,

    /// Canonical surface form plus variants ("Tuesday", "Tue", "tues").
    /// All forms decay if unused; the entry doesn't disappear until the
    /// element itself decays out. `Vec<String>` because one logical
    /// element legitimately has multiple labels.
    pub names: Vec<String>,

    /// Dynamic memory state — activation, salience, access counts, decay.
    /// Updated by the pipeline on every access; the same shape as
    /// `Relation.stats` because memory dynamics are uniform across
    /// substrate primitives.
    pub stats: MemoryStats,

    /// Tick this element was minted. Used for age-based decay and
    /// promotion-window calculations.
    pub created_at: Tick,

    /// Semantic anchor for this element. Populated at mint time from
    /// `names` (or the originating span text for anonymous NER spans).
    /// Read by region routing (Step 4) and anywhere similarity is needed.
    /// Inline `Vec<f32>` so a single index lookup gets you the vector
    /// without an extra hash hop.
    pub embedding: Vec<f32>,

    /// Semantic class — does this element carry content the downstream
    /// pipeline should propagate (`Signal`), or is it filler that
    /// matches without contributing (`Void`)? Inherited from parent at
    /// mint time so every region under the seeded `void` anchor (the
    /// closed-class grammatical regions — determiners, adpositions,
    /// auxiliaries, …) automatically marks its members for filtering.
    /// Routing uses the same cosine machinery for both classes; what
    /// changes is the disposition of the matched token downstream.
    pub polarity: Polarity,
}

/// Whether an element contributes content downstream (`Signal`) or
/// filters it out (`Void`). The closed-class grammatical regions
/// seeded under the `void` anchor — determiners, adpositions, and so
/// on — and every element under them carry `Void`; everything else
/// defaults to `Signal`. Not to be confused with routing-VOID, where
/// `unrouted_count` counts branches that failed to clear
/// `leaf_vigilance` regardless of element polarity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Polarity {
    /// Carries content the downstream pipeline should propagate.
    /// Default for newly minted elements.
    #[default]
    Signal,
    /// Filler — matches the input via cosine but is dropped from
    /// downstream extraction.
    Void,
}

/// A claim that something is true about one or more elements. Six
/// fields, no privileged predicate slot — the predicate is just an
/// attribute named e.g. `"is_a"` whose value is the claimed type. Meta-
/// relations (frame, source, supersession) are ordinary Relations whose
/// attribute list includes `Term::Relation(target)` pointing at the
/// relation they modify.
#[derive(Clone, Debug)]
pub struct Relation {
    /// Stable handle.
    pub id: RelationId,

    /// Named slots filling this relation, typically 2–5 entries. The
    /// attribute *name* is itself an Element (so renaming a role is a
    /// graph operation, not a type change).
    pub attributes: Vec<Attribute>,

    /// Where this relation sits in the assertion lifecycle. Filtering on
    /// status drives focus inclusion (Asserted/Entailed → focused;
    /// Defeasible → flagged; Superseded → history; Retracted → hidden).
    pub status: RelationStatus,

    /// Dynamic memory state. Same shape as `Element.stats`.
    pub stats: MemoryStats,

    /// Hand-set ordering knob, primarily for tie-breaking when two
    /// relations rank equal under salience/activation. Signed so it can
    /// nudge in either direction. Defaults to 0.
    pub priority: i8,

    /// Tick this relation was minted.
    pub created_at: Tick,
}

/// One named slot in a relation. The slot name is an Element (so the
/// vocabulary of attribute names is itself part of the graph), the
/// value is whatever fills it.
#[derive(Clone, Copy, Debug)]
pub struct Attribute {
    /// Element naming this slot (e.g. the element whose canonical name
    /// is `"SUBJECT"`, `"ACTOR"`, `"TARGET"`, `"target"` for meta-rels).
    pub name: ElementId,

    /// What fills the slot — concrete element or a nested relation
    /// reference (used by meta-relations to point at their parent).
    pub value: Term,
}

/// What an attribute slot can hold. Two variants is enough: most slots
/// reference an Element (the concrete filler), meta-relations reference
/// another Relation. No third "literal" variant — typed leaf values
/// like "Tuesday" are themselves Elements whose surface form lives in
/// `names` and whose semantics are parsed on comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Term {
    /// Concrete filler.
    Element(ElementId),

    /// Nested or meta-relation reference. Used by meta-relations on
    /// their `target` attribute to point at the parent relation.
    Relation(RelationId),
}

/// The lifecycle state of a Relation's claim on truth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelationStatus {
    /// Confirmed. In the active belief set; eligible for focus.
    Asserted,

    /// Derived from other Asserted/Entailed relations; not directly
    /// observed but believed to follow.
    Entailed,

    /// Provisional. Counts toward replay confirmation but flagged with
    /// `is_defeasible = true` in the frame and given lower base weight.
    /// Promotes to Asserted once `policy.promotion_min_count` /
    /// `promotion_min_diversity` thresholds are met within
    /// `promotion_window_ticks`.
    Defeasible,

    /// Replaced by a newer claim. Excluded from `focused_relations` but
    /// surfaced in the frame's `history` field for traceability.
    Superseded,

    /// Withdrawn entirely. Excluded from both focus and history.
    Retracted,
}

/// Dynamic memory state attached to every Element and Relation. Same
/// shape on both because memory dynamics (activation, decay, salience)
/// are uniform across substrate primitives.
#[derive(Default, Clone, Copy, Debug)]
pub struct MemoryStats {
    /// Current tick's spreading-activation level. Recomputed each tick
    /// during the focus walk; not durable across ticks (decays via
    /// `policy.decay_rate` outside the focus radius).
    pub activation: f32,

    /// Belief strength. Mints start mid-range; updated by replay
    /// confirmation, supersession, and explicit corrections.
    pub confidence: f32,

    /// Long-term durability. High = formative (easy to update, fast to
    /// decay if unused); low = settled (hard to update, slow to decay).
    /// Inverse of "stickiness" — fresh memories are plastic, old
    /// reinforced ones aren't.
    pub plasticity: f32,

    /// Amygdala-style protection accumulator. Boosted by emotional /
    /// surprising / high-stakes signals; floored by
    /// `policy.salience_floor`. Resists decay even when activation is
    /// low.
    pub salience: f32,

    /// Total times this element/relation has been read or activated.
    /// Used as a tiebreaker and a decay input (rarely-accessed things
    /// decay faster).
    pub access_count: u32,

    /// Bumped each tick this thing landed on the focus path *and* the
    /// tick was judged successful (per Step 10 hebbian reinforcement).
    /// Drives RRF rank in Step 12.
    pub focus_success_count: u32,

    /// Independent ticks supporting this claim. Distinct from
    /// `access_count`: this counts re-derivation events, not lookups.
    /// Used by the Defeasible → Asserted promotion gate.
    pub support_count: u32,

    /// Distinct evidence-source dimensions seen (e.g. distinct sources,
    /// distinct windows, distinct frames). Promotes alongside
    /// `support_count` to prevent single-source over-confidence.
    pub support_diversity: u32,

    /// Magnitude of the previous prediction's miss. Drives plasticity
    /// updates: a big surprise should make the relation more plastic.
    pub prediction_error: f32,

    /// Tick this thing last appeared in input or was directly mentioned.
    /// Distinct from `last_accessed` (which includes activation traversal).
    pub last_seen: Tick,

    /// Tick this thing was last touched by *any* access — input,
    /// activation walk, retrieval. `Option` because a freshly minted
    /// element may not have been accessed yet.
    pub last_accessed: Option<Tick>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Tick I/O
// ─────────────────────────────────────────────────────────────────────────────
/// The output of every tick. Most fields are *gathered* from per-tick
/// buffers that earlier steps populated as a side effect; Step 12's own
/// work is just the `focused_relations` RRF merge and `next_actions`
/// suggestions. The frame is a post-tick snapshot of the focused
/// subgraph, not a pre-assembled answer — the calling LLM derives any
/// natural-language response from the structural content.
#[derive(Debug)]
pub struct ConsciousAttentionFrame {
    /// Which Hypergraph clock value produced this frame.
    pub tick: Tick,

    /// Echo of the input text. Not durable.
    pub input_echo: String,

    /// 4-dim intent vector from Step 1 — cosine projection against the
    /// intent prototype banks. Drives Step 2's policy adjustments.
    pub intent: Intent,

    /// The active frame at the time this tick ran. `None` when no frame
    /// is active and the input doesn't establish one. Inherited from
    /// `recent_focus` or set by a frame-shifting cue in Step 4.
    pub active_frame: Option<ElementId>,

    /// Regions that lit up during routing. Union of per-window
    /// `route_regions(...)` results from Step 4.
    pub active_regions: Vec<RegionActivation>,

    /// The focused-relations ranked list, fused via RRF (Reciprocal
    /// Rank Fusion) over Dense (path-reinforced focus set), Sparse
    /// (BM25), and Path-reinforced (focus_success bumps from Step 10).
    /// RRF merges ranked lists by `Σ 1 / (60 + rank_i)` — keeps ranks,
    /// discards incompatibly-scaled raw scores. Asserted + Entailed
    /// pass; Defeasible flagged with lower weight; Superseded/Retracted
    /// excluded.
    pub focused_relations: Vec<RelationActivation>,

    /// For each focused R: meta-relations of `derived_from` / `source`
    /// type, walked via `meta_relations_by_subject` index. Lets the
    /// caller see *why* each focused relation is in the frame.
    pub supporting_claims: Vec<ClaimRef>,

    /// For each focused R: the supersession chain. Walked via
    /// `meta_relations_by_subject` filtered to `supersedes` attributes.
    /// Includes Superseded relations excluded from `focused_relations`.
    pub history: Vec<ClaimRef>,

    /// Per-tick uncertainty signals pushed by Steps 4, 5, 6, 8, 9.
    /// Step 12 collects from a per-tick buffer and surfaces them so the
    /// caller can act on disagreement, ungrounded time, ambiguous
    /// coreference, low-confidence extraction, or detected contradictions.
    pub uncertainty: Vec<UncertaintySignal>,

    /// ElementIds minted during this tick. Recorded by Steps 7 and 8
    /// into a per-tick write buffer. Overlaps with `focused_relations`
    /// in ID space — exists as a flat-list shortcut for "what did this
    /// tick change?" inspection.
    pub durable_writes: Vec<ElementId>,

    /// RelationIds whose status flipped to Superseded this tick.
    /// Recorded by Step 9. Same shortcut role as `durable_writes`.
    pub superseded: Vec<RelationId>,

    /// Advisories the caller can act on after the frame returns.
    /// `EnqueueReplay { kind }` schedules a background sweep;
    /// `FollowUpQuery(text)` suggests the agent ask a clarifying
    /// question.
    pub next_actions: Vec<AttentionAction>,
}

/// Type alias used in the frame for clarity — these are RelationIds
/// being treated as references to specific *claims*, not as plain
/// graph handles. Same compiler representation; just naming.
pub type ClaimRef = RelationId;

// ─────────────────────────────────────────────────────────────────────────────
// Policy — tuning knobs every pipeline step reads
// ─────────────────────────────────────────────────────────────────────────────

/// Tuning constants. The base Policy on the Hypergraph is the inter-tick
/// rest state; Step 2 produces a per-tick adjusted Policy from it
/// (using the intent from Step 1) that Steps 4–12 see. Tick-internal
/// subroutines read `&Policy`; only PFC writes the rest state.
#[derive(Clone, Debug)]
pub struct Policy {
    // ── Region routing ────────────────────────────────────────────────
    /// Cosine threshold below which routing stops descending into a
    /// region's children. Higher = stricter routing, fewer false
    /// matches; lower = broader retrieval.
    pub descend_threshold: f32,

    /// Vigilance for accepting a leaf match. Absolute (not relative to
    /// best alternative) so it acts as a quality floor regardless of
    /// the rest of the routing tree. Intent-driven — Step 2 may scale
    /// this up for high-precision intents.
    pub leaf_vigilance: f32,

    /// Similarity above which two prototypes in the same region merge
    /// into one. Prevents prototype proliferation.
    pub merge_threshold: f32,

    /// Within-region variance above which a region splits its
    /// prototypes into a new sub-region. Shapes the granularity of the
    /// region DAG.
    pub split_variance: f32,

    /// Cosine threshold below which routing reports the input as
    /// "void" — no region matches well enough. Triggers a
    /// `DiffuseRouting` uncertainty signal.
    pub void_threshold: f32,

    /// Region activation level above which a region counts as "active"
    /// in the frame's `active_regions`.
    pub region_activation_threshold: f32,

    /// Variance prior (ε) added to every per-dimension variance during
    /// diagonal-Mahalanobis routing in Step 4. Two roles:
    /// 1. Numerical stability — prevents divide-by-zero when a region
    ///    has only one prototype (variance = 0).
    /// 2. Bootstrap smoothing — when n is small, the empirical variance
    ///    is unreliable, and ε mixes in a "background uncertainty"
    ///    floor. With n = 20 seed prototypes the prior is small enough
    ///    not to dominate; with n = 1 (a newly-minted mid-path region)
    ///    it dominates and routing falls back to centroid-cosine.
    pub variance_prior: f32,

    /// Number of top-scoring prototypes per region averaged into the
    /// cosine score (option 2 of §4 fusion notes). Replaces the
    /// previous max-pool with mean-of-top-K so a single surface-
    /// overlap prototype can't dominate a region's score — requires
    /// prototype agreement before a region ranks high. K = 3 is a
    /// reasonable default for 20-prototype seed regions; raise it to
    /// require broader agreement, lower to recover max-pool behavior
    /// (K = 1 reproduces max exactly).
    pub cosine_top_k: u32,

    // ── Attribute-name dedup ─────────────────────────────────────────
    /// Cosine similarity threshold for collapsing two attribute-name
    /// elements into one (Step 8 / §11.9). Strict by default so
    /// `SUBJECT` and `ACTOR` stay distinct even though they overlap.
    pub attribute_name_dedup_threshold: f32,

    /// Threshold (per-tick attribute-name mint count) above which the
    /// tick is flagged for priority replay-dedup. Bursts of new
    /// attribute names usually mean drift, not legitimate vocabulary
    /// growth.
    pub attribute_name_mint_warning_count: u32,

    // ── Mid-path DAG insertion ───────────────────────────────────────
    /// Confidence-gap threshold for confirming a mid-path region
    /// insertion. Tuned small so insertion is aggressive.
    pub midpath_confirm_gap: f32,

    /// Independent-evidence threshold for the same — ticks of
    /// supporting signals before a mid-path candidate becomes a
    /// permanent region.
    pub midpath_confirm_evidence: u32,

    /// Confidence-gap threshold for *moving* (re-parenting) an existing
    /// region in the DAG. Higher bar than insertion because the move
    /// invalidates more downstream state.
    pub midpath_reparent_gap: f32,

    // ── Defeasible → Asserted gate ───────────────────────────────────
    /// Minimum independent ticks supporting a Defeasible relation
    /// before it promotes to Asserted.
    pub promotion_min_count: u32,

    /// Minimum distinct evidence-source dimensions among those
    /// supporting ticks. Prevents single-source promotion.
    pub promotion_min_diversity: u32,

    /// Window of ticks within which the count/diversity must accumulate.
    /// Older support doesn't carry forward.
    pub promotion_window_ticks: u32,

    // ── Recognition thresholds ───────────────────────────────────────
    /// Co-occurrence count above which a co-occurrence pattern is
    /// recognized as a concept.
    pub concept_recognition_threshold: u32,

    /// Stricter version of the same for frame recognition — frames are
    /// higher-stakes structures, so the bar is higher.
    pub frame_recognition_threshold: u32,

    // ── Memory dynamics ──────────────────────────────────────────────
    /// Per-tick decay applied outside the focus radius by the
    /// background sweep (§14.7).
    pub decay_rate: f32,

    /// Lower bound on `MemoryStats.salience`. Salience can climb but
    /// never falls below this floor while the element/relation is
    /// retained.
    pub salience_floor: f32,

    /// Hebbian learning rate — how much focus-co-occurrence bumps
    /// `focus_success_count` and related stats. Intent-modulated by
    /// Step 2.
    pub hebbian_rate: f32,

    /// Number of hops out from the focus path that escapes decay this
    /// tick.
    pub focus_decay_radius: u32,

    /// Capacity of `Hypergraph.recent_focus`.
    pub recent_focus_capacity: u32,

    /// Minimum focus-success count for a relation to be eligible for
    /// replay reinforcement.
    pub replay_focus_floor: u32,

    // ── Extractor confidence threshold ───────────────────────────────
    /// NER assertion-confidence threshold. Below this, Step 5 mints the
    /// relation as `Defeasible` rather than `Asserted`.
    pub ner_assertion_threshold: f32,

    // ── Coref ────────────────────────────────────────────────────────
    /// Minimum aggregate score for a Step 6 coref decision to fire.
    /// Below this, the ambiguous span falls through to Step 8's
    /// normal mint path. Tuned conservatively (default 1.0) so a
    /// recency bonus alone isn't enough to bind — coref needs at
    /// least one substantive signal on top. Wrong binds are harder
    /// to unwind than missed binds (replay can merge later).
    pub coref_threshold: f32,

    // ── Intent-modulated knobs (Step 2 outputs) ──────────────────────
    //
    // These five fields hold the rest-state base on the Hypergraph's
    // Policy and the adjusted scalar on the per-tick Policy that Step 2
    // returns. Same field name, two roles — Step 2 reads each as
    // `base_*` and writes back the §10.6-formula result on the
    // returned Policy. Steps 4–12 only ever see the adjusted view.
    /// Default confidence assigned to newly-minted Asserted relations
    /// before extractor confidence multiplies it (§11.9, §3420).
    /// Formula: `default_conf = base_conf * conviction * (1 - 0.7 * curiosity)`.
    /// Confident statements raise it; questions / hedges drop it.
    pub default_conf: f32,

    /// Multiplier applied to per-tick salience bumps in the hebbian
    /// reinforcement step (§11.11). Formula:
    /// `salience_multiplier = base_salience + arousal + prediction_error`.
    /// Surprise + emotional intensity → harder encoding.
    pub salience_multiplier: f32,

    /// Threshold above which Step 9 actively searches for and marks
    /// prior relations as Superseded. Formula:
    /// `supersession_threshold = base_threshold * (1 - prediction_error)`.
    /// High prediction-error inputs lower the bar so contradictions
    /// trigger lookup; low-PE inputs leave the cache alone.
    pub supersession_threshold: f32,

    // ── Replay ───────────────────────────────────────────────────────
    /// Replay scheduling mode. Currently a single-variant placeholder
    /// pending the replay subsystem's design.
    pub replay_cadence: ReplayCadence,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            // Option-D fusion: cosine drives descent + leaf_vigilance
            // (its wide dynamic range is decisive at sharp boundaries),
            // Mahalanobis drives region_activation_threshold (its
            // distribution-awareness focuses the active set on regions
            // the input actually fits). Thresholds are interpreted in
            // the respective metric's scale.
            descend_threshold: 0.05,
            leaf_vigilance: 0.05,
            merge_threshold: 0.0,
            split_variance: 0.0,
            void_threshold: 0.0,
            region_activation_threshold: 0.55,
            variance_prior: 0.001,
            cosine_top_k: 3,

            attribute_name_dedup_threshold: 0.85,
            attribute_name_mint_warning_count: 5,

            midpath_confirm_gap: 0.05,
            midpath_confirm_evidence: 3,
            midpath_reparent_gap: 0.10,

            promotion_min_count: 3,
            // v0 leaves diversity at 0: `support_diversity` is replay's
            // job (§14.7/§14.8) and v0 has no replay loop, so a non-zero
            // gate would permanently block promotion. Replay flips this
            // upward once it lands.
            promotion_min_diversity: 0,
            promotion_window_ticks: 1000,

            concept_recognition_threshold: 3,
            frame_recognition_threshold: 5,

            decay_rate: 0.0,
            salience_floor: 0.0,
            hebbian_rate: 0.0,
            focus_decay_radius: 0,
            recent_focus_capacity: 64,
            replay_focus_floor: 3,

            ner_assertion_threshold: 0.7,

            coref_threshold: 1.0,

            // §10.6 base values. `default_conf = 1.0` lines up with the
            // worked examples (high-conviction → ~0.90, hedged → ~0.09).
            // The other two are mid-range placeholders pending §19
            // calibration.
            default_conf: 1.0,
            salience_multiplier: 1.0,
            // v0 has no replay loop to repair over-eager flips. Even so,
            // 0.5 was over-conservative: the gate compounds with
            // `default_conf`'s conviction multiplier (line 695) and with
            // the per-tick `* (1 - PE)` modulation in §10.6, so events
            // that should obviously supersede ("Bob moved to Austin"
            // following a "Bob lives in Boston" cache) routinely sat
            // below 0.5. 0.35 lets typical declarative change-events
            // flip while still gating questions / hedges out.
            supersession_threshold: 0.35,

            replay_cadence: ReplayCadence::default(),
        }
    }
}

/// Replay scheduling mode. The doc references this on `Policy` but
/// never enumerates the variants — single placeholder for now. Expand
/// when the replay subsystem (§14.7 background sweep + downstream)
/// gets concrete.
#[derive(Clone, Copy, Debug, Default)]
pub enum ReplayCadence {
    #[default]
    Default,
}

// ─────────────────────────────────────────────────────────────────────────────
// Working memory & per-tick activation snapshots
// ─────────────────────────────────────────────────────────────────────────────

/// One entry in `Hypergraph.recent_focus`. Step 4 walks this ring
/// looking for an active frame to inherit when the input doesn't
/// establish one of its own.
#[derive(Clone, Copy, Debug)]
pub struct RecentFocusEntry {
    /// The focal element — what was attended to.
    pub element: ElementId,

    /// The attribute name binding `element` on its most recent focus
    /// (e.g. SUBJECT, ACTOR, TARGET). `None` when the element was
    /// focal but not in a recognized attribute role.
    pub attribute: Option<ElementId>,

    /// The active frame at the time this entry was pushed. Lets later
    /// ticks distinguish "X was focused inside frame F" from "X was
    /// focused frame-free".
    pub frame: Option<ElementId>,

    /// Tick this entry entered focus. Used to age out stale entries.
    pub tick: Tick,
}

/// 4-dim intent vector produced by Step 1. Each component is a cosine
/// projection of the input's window embedding against an intent
/// prototype bank (the four banks come from the seed pack). Drives
/// Step 2's policy adjustments. Maps onto neuromodulator analogs:
/// conviction (cognitive), prediction_error (DA), arousal (NE),
/// curiosity (retrieval vs assertion shape). Does NOT gate which
/// steps run — only modulates how Steps 4/8/10/11 weight their work.
#[derive(Default, Clone, Copy, Debug)]
pub struct Intent {
    /// Speaker certainty / commitment of the assertion. High =
    /// confident statements that should land as `Asserted` more
    /// readily; low = hedged statements that lean `Defeasible`.
    pub conviction: f32,

    /// Novelty / contradiction signal — how much this input clashes
    /// with existing claims. Dopamine analog. Drives plasticity bumps
    /// and salience for surprising inputs.
    pub prediction_error: f32,

    /// Emotional intensity / importance. Norepinephrine analog. Lifts
    /// salience and slows decay for the elements/relations involved.
    pub arousal: f32,

    /// Retrieval-shape vs assertion-shape. High = "what do I know
    /// about X?" (retrieval-leaning); low = "X is true" (assertion-
    /// leaning). Modulates how aggressively Step 12 surfaces history
    /// and supporting claims.
    pub curiosity: f32,
}

/// One region that lit up during Step 4 routing.
#[derive(Clone, Copy, Debug)]
pub struct RegionActivation {
    /// The region anchor element.
    pub region: ElementId,

    /// Activation level — how strongly this region matched the input.
    /// Above `policy.region_activation_threshold` to land in the frame.
    pub activation: f32,
}

/// Per-dimension mean + variance of a region's prototype embeddings.
/// Populated by `seed::rebuild_indices` after `region_prototypes` is
/// built; consumed by Step 4 routing for diagonal Mahalanobis scoring.
///
/// Length invariant: `mean.len() == var.len() == EMBEDDING_DIM` (384).
/// With `n == 1` the variance is all-zeros (a single point has no
/// spread); Step 4 applies a variance prior at use time so the
/// Mahalanobis denominator never collapses.
///
/// `PartialEq` is derived for the idempotency test in `seed::tests` —
/// rebuild_indices over identical inputs yields bit-identical stats
/// because the arithmetic is deterministic in iteration order.
#[derive(Clone, Debug, PartialEq)]
pub struct RegionStats {
    /// Per-dimension mean of the prototype embeddings.
    pub mean: Vec<f32>,

    /// Per-dimension variance of the prototype embeddings.
    /// Computed as `Σ(xᵢ − μ)² / n` (population variance — we treat
    /// the prototype set as the full population for this region, not
    /// a sample drawn from a larger population).
    pub var: Vec<f32>,

    /// Number of prototype embeddings folded into mean/var. Step 4
    /// uses this to weight the variance prior — small `n` → trust the
    /// prior more; large `n` → trust the empirical variance.
    pub n: u32,
}

/// What Step 4 routing proposes to do to the substrate. Held through
/// the read-mostly phase, applied by Step 7 (`apply_region_delta`)
/// during the mutation phase. Verbatim from §10.2 of `new_foundation.md`.
#[derive(Default, Debug, Clone)]
pub struct RegionDelta {
    /// `(child, parent, weight)` — committed as new
    /// `(child, parent_region, parent)` relations with
    /// `stats.confidence = weight`, or as confidence reinforcement on
    /// existing `parent_region` relations.
    pub parent_attachments: Vec<(ElementId, ElementId, f32)>,

    /// `(prototype_element, target_embedding)` — committed by
    /// overwriting the prototype Element's inline `embedding` via
    /// spherical k-means: `new = normalize(old + lr · (target − old))`
    /// where `lr = proto.stats.plasticity * policy.hebbian_rate`.
    /// Step 4 stores only the target; Step 7 reads policy + plasticity
    /// and does the math.
    pub prototype_updates: Vec<(ElementId, Vec<f32>)>,

    /// Empty in v0 Step 4 — mid-path insertion (§10.3.5) needs span-
    /// level embeddings, which only exist after Step 6 mints elements.
    /// Step 8 owns this list. Field exists so the struct stays stable
    /// across pipeline phases.
    pub new_regions: Vec<NewRegion>,

    /// Empty in v0 Step 4 for the same reason — authoritative member
    /// assignment uses each minted element's own embedding (Step 8).
    pub new_members: Vec<(ElementId, ElementId)>,

    /// Number of routing branches that died at sub-threshold quality
    /// (best child below `policy.leaf_vigilance`). Surfaces in the
    /// frame as a routing-quality signal. Not to be confused with
    /// elements whose `polarity == Polarity::Void` — that's a
    /// semantic-content classification, this is a routing failure.
    pub unrouted_count: u32,
}

/// Mid-path insertion candidate. Populated by Step 8 from span-level
/// embeddings; written by Step 7 as a Defeasible `parent_region`
/// relation pending replay confirmation. Not used in v0 Step 4.
#[derive(Debug, Clone)]
pub struct NewRegion {
    pub parent: ElementId,
    pub initial_prototype: Vec<f32>,
}

/// One relation in the frame's ranked focus list. `activation`
/// carries the Step 12 RRF-fused score (NOT the raw
/// `stats.activation` post-decay — those drive the RRF input
/// ranking, not the output). `is_defeasible` mirrors
/// `status == Defeasible` so callers can filter without an
/// extra lookup; consumers typically present Defeasible relations
/// with reduced weight or in a separate tray.
#[derive(Clone, Copy, Debug)]
pub struct RelationActivation {
    pub relation: RelationId,
    pub activation: f32,
    pub is_defeasible: bool,
}

// ─────────────────────────────────────────────────────────────────────────────
// Uncertainty & advisory signals
// ─────────────────────────────────────────────────────────────────────────────

/// One detected uncertainty signal. The pipeline pushes these into a
/// per-tick buffer as side effects of Steps 4, 5, 6, 8, and 9; Step 12
/// collects them into the frame's `uncertainty` field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UncertaintySignal {
    /// Step 4: routing didn't converge — multiple regions matched
    /// equivalently, or none matched above `void_threshold`.
    DiffuseRouting,

    /// Step 5/6: a temporal reference ("yesterday", "next Tuesday")
    /// couldn't be resolved against `Input.wall_clock` plus context.
    UngroundedTime,

    /// Step 6: coreference resolution found multiple equally-plausible
    /// candidates for a referring expression.
    AmbiguousCoref,

    /// Step 5/8: an extractor produced output below
    /// `policy.ner_assertion_threshold` (relation minted as
    /// `Defeasible`, signal raised so the caller knows).
    LowConfidence,

    /// Step 9: a candidate supersession lacks a clear winner —
    /// the new claim and the existing one conflict but neither
    /// dominates. The caller may want to ask a follow-up.
    Contradiction,
}

/// An advisory action Step 12 emits in the frame's `next_actions`.
/// The caller (typically the agent loop wrapping `tick()`) decides
/// whether to act on each.
#[derive(Clone, Debug)]
pub enum AttentionAction {
    /// Schedule a background replay sweep of the given kind. Doesn't
    /// run inline — the caller hands this off to the replay thread.
    EnqueueReplay { kind: ReplayKind },

    /// Suggest the agent ask a clarifying follow-up question. Carries
    /// the suggested prompt text as raw `String`.
    FollowUpQuery(String),
}

/// What kind of background work to enqueue. Doc only names
/// `BackgroundSweep` (the §14.7 decay sweep) — placeholder for now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayKind {
    /// §14.7 decay sweep over everything outside the focus radius.
    BackgroundSweep,
}
