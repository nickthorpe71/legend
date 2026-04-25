//! Concept classifier framework — Thousand-Brains-style column modules.
//!
//! Each `ConceptClassifier` owns a bounded model that votes on whether
//! incoming text expresses a particular concept (decision, bug,
//! preference, …). Classifiers see a shared `ClassificationContext`
//! (text + keyword cache today; graph/embedding later) and emit
//! evidence-weighted `ConceptVote`s.
//!
//! Phase 1 (this commit) ships the trait, the registry, and three
//! starter classifiers that wrap Legend's existing keyword logic. The
//! votes are observable but **do not drive salience or routing yet** —
//! that's a follow-on once we have more classifiers and want to
//! consensus their output (queue item #34's broader vision).
//!
//! Background: ARCHITECTURE tick from this repo's memory —
//! "Thousand Brains research maps cleanly onto Legend as column-like
//! learning modules, not as more biology labels. Each module should
//! own a bounded reference-frame-local model, emit evidence-weighted
//! votes for hypotheses/context/goals, and communicate only compact
//! votes through a common protocol."

use crate::memory::wernicke::KeywordCache;

/// Compact vote a classifier emits when it recognizes its concept.
/// Confidence is in [0, 1]; evidence is the surface-level cues that
/// fired so consumers can trace the decision back.
#[derive(Debug, Clone)]
#[allow(dead_code)] // framework surface; fields are read by tests + future consumers
pub struct ConceptVote {
    pub concept: String,
    pub confidence: f32,
    pub evidence: Vec<String>,
    /// Optional reference frame (e.g. project name) that scopes the
    /// vote. `None` means domain-general.
    pub reference_frame: Option<String>,
}

/// Read-only context handed to each classifier. Adding fields here is
/// the path to give classifiers richer signal (graph state, embedding,
/// neurochemistry); keep additions backward-compatible.
pub struct ClassificationContext<'a> {
    pub text: &'a str,
    pub keyword_cache: &'a KeywordCache,
}

/// A column-like classifier. Implementations must be `Send + Sync` so
/// the registry can live behind `LazyLock` if a future caller wants
/// it process-wide.
#[allow(dead_code)] // `id` is part of the public framework contract
pub trait ConceptClassifier: Send + Sync {
    /// Stable identifier used for logging and observability (e.g.
    /// `"decision"`, `"bug"`). Should be unique per classifier kind.
    fn id(&self) -> &str;

    /// Evaluate `ctx` and return a vote if the classifier sees its
    /// concept; `None` otherwise. Returning `None` is cheaper than a
    /// vote with `confidence: 0` because callers can short-circuit.
    fn vote(&self, ctx: &ClassificationContext<'_>) -> Option<ConceptVote>;
}

/// Container for the active classifier set. Vote order matches insert
/// order so dependents (consensus, observability) can rely on stable
/// indices across runs.
#[derive(Default)]
pub struct ConceptRegistry {
    classifiers: Vec<Box<dyn ConceptClassifier>>,
}

impl ConceptRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, classifier: Box<dyn ConceptClassifier>) {
        self.classifiers.push(classifier);
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.classifiers.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.classifiers.is_empty()
    }

    /// Run every classifier against `ctx` and collect the votes.
    pub fn vote_all(&self, ctx: &ClassificationContext<'_>) -> Vec<ConceptVote> {
        let mut out = Vec::with_capacity(self.classifiers.len());
        for c in &self.classifiers {
            if let Some(v) = c.vote(ctx) {
                out.push(v);
            }
        }
        out
    }

    /// Build the default registry shipped with Legend. Keep this list
    /// small and grounded in existing keyword logic — the framework
    /// is meant to grow toward learned, reference-frame-aware
    /// modules over time, not to ossify the current keyword shape.
    pub fn default_set() -> Self {
        let mut r = Self::new();
        r.register(Box::new(DecisionClassifier));
        r.register(Box::new(BugClassifier));
        r.register(Box::new(PreferenceClassifier));
        r
    }
}

// ---------------------------------------------------------------------------
// Starter classifiers — wrap the existing keyword cues.
// ---------------------------------------------------------------------------

/// Votes when decision-language is present. Two or more cues raise the
/// confidence ceiling; a single cue with rationale language ("because",
/// "rationale", "reason") behaves as a strong signal too.
pub struct DecisionClassifier;

impl ConceptClassifier for DecisionClassifier {
    fn id(&self) -> &str {
        "decision"
    }

    fn vote(&self, ctx: &ClassificationContext<'_>) -> Option<ConceptVote> {
        let lowered = ctx.text.to_lowercase();
        let hits: Vec<String> = ctx
            .keyword_cache
            .decision
            .iter()
            .filter(|k| lowered.contains(k.as_str()))
            .cloned()
            .collect();
        if hits.is_empty() {
            return None;
        }
        let rationale_present =
            lowered.contains("because") || lowered.contains("rationale") || lowered.contains("reason");
        let confidence = match (hits.len(), rationale_present) {
            (1, false) => 0.4,
            (1, true) => 0.7,
            (n, _) if n >= 2 => 0.85_f32.min(0.5 + 0.1 * n as f32),
            _ => 0.4,
        };
        Some(ConceptVote {
            concept: "decision".into(),
            confidence,
            evidence: hits,
            reference_frame: None,
        })
    }
}

/// Votes when bug/incident language is present.
pub struct BugClassifier;

impl ConceptClassifier for BugClassifier {
    fn id(&self) -> &str {
        "bug"
    }

    fn vote(&self, ctx: &ClassificationContext<'_>) -> Option<ConceptVote> {
        let lowered = ctx.text.to_lowercase();
        let hits: Vec<String> = ctx
            .keyword_cache
            .bug
            .iter()
            .filter(|k| lowered.contains(k.as_str()))
            .cloned()
            .collect();
        if hits.is_empty() {
            return None;
        }
        // Single bug-keyword hit: 0.45. Each additional hit adds 0.1
        // up to a 0.85 ceiling (room for graph/contradiction signals
        // to push confidence higher in a future phase).
        let confidence = (0.35 + 0.1 * hits.len() as f32).min(0.85);
        Some(ConceptVote {
            concept: "bug".into(),
            confidence,
            evidence: hits,
            reference_frame: None,
        })
    }
}

/// Votes when preference / convention / rule language is present.
pub struct PreferenceClassifier;

impl ConceptClassifier for PreferenceClassifier {
    fn id(&self) -> &str {
        "preference"
    }

    fn vote(&self, ctx: &ClassificationContext<'_>) -> Option<ConceptVote> {
        let lowered = ctx.text.to_lowercase();
        let hits: Vec<String> = ctx
            .keyword_cache
            .preference
            .iter()
            .filter(|k| lowered.contains(k.as_str()))
            .cloned()
            .collect();
        if hits.is_empty() {
            return None;
        }
        let confidence = (0.35 + 0.08 * hits.len() as f32).min(0.8);
        Some(ConceptVote {
            concept: "preference".into(),
            confidence,
            evidence: hits,
            reference_frame: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn kw() -> KeywordCache {
        KeywordCache::default_from_static()
    }

    #[test]
    fn decision_classifier_fires_on_decision_keyword() {
        let kw = kw();
        let ctx = ClassificationContext {
            text: "DECISION: chose tokio because broader ecosystem",
            keyword_cache: &kw,
        };
        let v = DecisionClassifier.vote(&ctx).expect("vote");
        assert_eq!(v.concept, "decision");
        assert!(v.confidence > 0.5, "confidence={}", v.confidence);
        assert!(!v.evidence.is_empty());
    }

    #[test]
    fn classifier_returns_none_when_no_signal() {
        let kw = kw();
        let ctx = ClassificationContext {
            text: "the plain weather report has nothing actionable",
            keyword_cache: &kw,
        };
        assert!(DecisionClassifier.vote(&ctx).is_none());
        assert!(BugClassifier.vote(&ctx).is_none());
        assert!(PreferenceClassifier.vote(&ctx).is_none());
    }

    #[test]
    fn registry_collects_votes_from_all_matchers() {
        let kw = kw();
        // Mixed text: decision rationale + a bug mention + a preference.
        let ctx = ClassificationContext {
            text: "DECISION: we prefer tokio because ecosystem; the panic in our async runtime was a bug",
            keyword_cache: &kw,
        };
        let r = ConceptRegistry::default_set();
        let votes = r.vote_all(&ctx);
        let ids: Vec<_> = votes.iter().map(|v| v.concept.as_str()).collect();
        assert!(ids.contains(&"decision"), "votes={:?}", ids);
        assert!(ids.contains(&"bug"), "votes={:?}", ids);
        assert!(ids.contains(&"preference"), "votes={:?}", ids);
    }

    #[test]
    fn registry_default_set_has_three_starter_classifiers() {
        let r = ConceptRegistry::default_set();
        assert_eq!(r.len(), 3);
    }
}
