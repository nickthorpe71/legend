/// Amygdala — emotional valence computation via embedding-space prototypes.
///
/// Replaces the old keyword-weight approach with semantic similarity against
/// ~100 pre-embedded anchor sentences. This makes valence scoring domain-
/// independent: "my dog died" registers as negative even without programming
/// keywords, because MiniLM understands its semantic proximity to "something
/// died or was destroyed."
///
/// Design:
/// - Each prototype has a pre-computed 384-dim embedding (generated once at
///   build time, hardcoded in `prototype_embeddings.rs`).
/// - Scoring: top-K most-similar prototypes vote via weighted average
///   (similarity × valence × confidence). Urgency prototypes amplify magnitude.
/// - The `confidence` field (starts at 1.0) enables future reinforcement
///   learning — validated prototypes gain weight, unused ones decay.
use super::entorhinal::cosine_similarity;
use super::prototype_embeddings;
use super::wernicke::KeywordCache;
use serde::{Deserialize, Serialize};

/// Number of most-similar prototypes that vote on valence.
const VALENCE_TOP_K: usize = 5;

/// Minimum cosine similarity for a prototype to contribute to the vote.
const VALENCE_MIN_SIMILARITY: f32 = 0.15;

/// Urgency magnitude amplification factor per urgency hit in top-K.
const URGENCY_AMPLIFICATION: f32 = 0.15;

/// An emotional anchor point in embedding space.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmotionalPrototype {
    /// Pre-computed 384-dim embedding of the anchor sentence.
    pub embedding: Vec<f32>,
    /// Hand-assigned base valence, adjusted by learning. Range [-1.0, 1.0].
    pub valence: f32,
    /// Learned confidence weight. Starts at 1.0, adjusted by reinforcement.
    pub confidence: f32,
    /// Category: "negative", "positive", "urgency", "neutral".
    pub category: String,
    /// Human-readable label (the anchor sentence text).
    pub label: String,
}

/// (sentence, valence, category) tuples for seeding prototypes.
/// Order must match `prototype_embeddings::SEED_EMBEDDINGS` 1:1.
pub const SEED_PROTOTYPES: &[(&str, f32, &str)] = &[
    // === NEGATIVE (-0.5 to -0.9): threat, loss, failure, frustration ===
    ("this is a complete disaster", -0.8, "negative"),
    ("everything is broken and nothing works", -0.7, "negative"),
    (
        "we lost important data and cannot recover it",
        -0.9,
        "negative",
    ),
    ("this keeps failing no matter what I try", -0.6, "negative"),
    ("something died or was destroyed", -0.8, "negative"),
    (
        "there is a serious security vulnerability",
        -0.7,
        "negative",
    ),
    (
        "the deadline was missed and we failed to deliver",
        -0.6,
        "negative",
    ),
    (
        "this error is catastrophic and unrecoverable",
        -0.9,
        "negative",
    ),
    (
        "the system crashed and corrupted the data",
        -0.8,
        "negative",
    ),
    ("we have a critical production outage", -0.8, "negative"),
    (
        "this is a terrible mistake with serious consequences",
        -0.7,
        "negative",
    ),
    ("performance has degraded severely", -0.5, "negative"),
    (
        "the project was cancelled after months of work",
        -0.7,
        "negative",
    ),
    (
        "trust has been broken and damage was done",
        -0.7,
        "negative",
    ),
    (
        "this situation is hopeless and getting worse",
        -0.8,
        "negative",
    ),
    ("a major regression was introduced", -0.6, "negative"),
    (
        "we are losing users and revenue is dropping",
        -0.6,
        "negative",
    ),
    (
        "the integration is completely incompatible",
        -0.5,
        "negative",
    ),
    (
        "someone made an unauthorized destructive change",
        -0.7,
        "negative",
    ),
    (
        "this vulnerability exposes sensitive user data",
        -0.8,
        "negative",
    ),
    ("the entire test suite is failing", -0.6, "negative"),
    ("we discovered a fundamental design flaw", -0.7, "negative"),
    (
        "resources are exhausted and the system is unresponsive",
        -0.6,
        "negative",
    ),
    (
        "there was a painful loss that cannot be undone",
        -0.9,
        "negative",
    ),
    (
        "this failure affects all downstream systems",
        -0.7,
        "negative",
    ),
    // === POSITIVE (+0.5 to +0.9): success, discovery, completion ===
    ("this is a major breakthrough", 0.8, "positive"),
    ("everything works perfectly now", 0.7, "positive"),
    ("finally solved it after a long struggle", 0.8, "positive"),
    ("successfully shipped and users are happy", 0.7, "positive"),
    ("made significant progress today", 0.5, "positive"),
    ("discovered an elegant solution", 0.7, "positive"),
    (
        "all tests are passing and coverage improved",
        0.5,
        "positive",
    ),
    (
        "the migration completed without any issues",
        0.5,
        "positive",
    ),
    ("performance improved dramatically", 0.6, "positive"),
    ("received great feedback from users", 0.6, "positive"),
    (
        "the refactor made everything cleaner and simpler",
        0.6,
        "positive",
    ),
    ("achieved a new personal best", 0.7, "positive"),
    ("the team delivered ahead of schedule", 0.6, "positive"),
    ("a difficult problem was finally resolved", 0.7, "positive"),
    ("this is exactly what we needed", 0.5, "positive"),
    ("the results exceeded all expectations", 0.7, "positive"),
    ("we reached an important milestone", 0.6, "positive"),
    ("the system is stable and reliable now", 0.5, "positive"),
    (
        "got accepted and recognized for the achievement",
        0.8,
        "positive",
    ),
    (
        "this collaboration produced amazing results",
        0.7,
        "positive",
    ),
    (
        "the optimization reduced costs significantly",
        0.6,
        "positive",
    ),
    ("everything came together beautifully", 0.7, "positive"),
    ("the fix resolved the issue permanently", 0.6, "positive"),
    (
        "we successfully recovered from the incident",
        0.6,
        "positive",
    ),
    ("this is a moment of celebration and pride", 0.8, "positive"),
    // === URGENCY: magnitude amplifiers ===
    ("this is blocking all other work", 0.0, "urgency"),
    ("we need to fix this immediately", 0.0, "urgency"),
    ("this is the highest priority right now", 0.0, "urgency"),
    (
        "there is no time left and the deadline is now",
        0.0,
        "urgency",
    ),
    ("this requires immediate attention", 0.0, "urgency"),
    (
        "everything else must stop until this is resolved",
        0.0,
        "urgency",
    ),
    ("this is an emergency that needs action now", 0.0, "urgency"),
    ("we cannot proceed until this is addressed", 0.0, "urgency"),
    ("this is time-sensitive and cannot wait", 0.0, "urgency"),
    ("critical path item that blocks the release", 0.0, "urgency"),
    ("the situation is escalating rapidly", 0.0, "urgency"),
    ("this needs to be done before anything else", 0.0, "urgency"),
    ("zero tolerance for delay on this issue", 0.0, "urgency"),
    ("stakeholders are waiting and escalating", 0.0, "urgency"),
    ("every minute of delay costs us more", 0.0, "urgency"),
    (
        "this has been escalated to the highest level",
        0.0,
        "urgency",
    ),
    ("immediate rollback is required", 0.0, "urgency"),
    (
        "production is down and customers are affected",
        0.0,
        "urgency",
    ),
    ("this cannot be deferred or deprioritized", 0.0, "urgency"),
    ("drop everything and focus on this", 0.0, "urgency"),
    (
        "the clock is ticking and we are running out of time",
        0.0,
        "urgency",
    ),
    (
        "this is a showstopper that must be fixed now",
        0.0,
        "urgency",
    ),
    ("we are in crisis mode", 0.0, "urgency"),
    ("this requires an all-hands response", 0.0, "urgency"),
    ("time-critical issue with customer impact", 0.0, "urgency"),
    // === NEUTRAL (0.0): factual, routine, descriptive ===
    ("updated the configuration settings", 0.0, "neutral"),
    ("moved the file to a different directory", 0.0, "neutral"),
    ("reviewed the documentation", 0.0, "neutral"),
    ("renamed the variable for clarity", 0.0, "neutral"),
    ("added a comment explaining the logic", 0.0, "neutral"),
    (
        "reformatted the code to match style guidelines",
        0.0,
        "neutral",
    ),
    ("created a new branch for the feature", 0.0, "neutral"),
    ("updated the version number", 0.0, "neutral"),
    ("adjusted the indentation and whitespace", 0.0, "neutral"),
    ("merged the branch into main", 0.0, "neutral"),
    ("set up the development environment", 0.0, "neutral"),
    ("changed the default parameter value", 0.0, "neutral"),
    ("listed the project dependencies", 0.0, "neutral"),
    ("organized files into separate folders", 0.0, "neutral"),
    ("wrote a summary of the meeting notes", 0.0, "neutral"),
    ("scheduled a follow-up discussion", 0.0, "neutral"),
    ("checked the status of the build", 0.0, "neutral"),
    ("ran the standard test suite", 0.0, "neutral"),
    ("switched between different task contexts", 0.0, "neutral"),
    ("archived old records for reference", 0.0, "neutral"),
    ("looked up the API documentation", 0.0, "neutral"),
    ("read through the changelog entries", 0.0, "neutral"),
    ("backed up the current state of the project", 0.0, "neutral"),
    ("noted a minor observation for later", 0.0, "neutral"),
    ("logged the routine maintenance activity", 0.0, "neutral"),
];

/// Build the initial set of emotional prototypes from hardcoded data.
/// Uses pre-computed embeddings — no ONNX inference needed.
pub fn seed_emotional_prototypes() -> Vec<EmotionalPrototype> {
    let embeddings = prototype_embeddings::SEED_EMBEDDINGS;
    debug_assert_eq!(
        SEED_PROTOTYPES.len(),
        embeddings.len(),
        "SEED_PROTOTYPES and SEED_EMBEDDINGS must have the same length"
    );

    SEED_PROTOTYPES
        .iter()
        .zip(embeddings.iter())
        .map(|(&(label, valence, category), emb)| EmotionalPrototype {
            embedding: emb.to_vec(),
            valence,
            confidence: 1.0,
            category: category.to_string(),
            label: label.to_string(),
        })
        .collect()
}

/// Compute emotional valence using embedding-space prototypes.
///
/// Takes the pre-computed embedding of the input text (already available from
/// the batch embed step in tick_impl — zero extra ONNX inference).
///
/// Algorithm:
/// 1. Cosine similarity against all prototypes
/// 2. Take top-K most similar (above minimum threshold)
/// 3. Weighted average: sum(sim × valence × confidence) / sum(sim × confidence)
/// 4. Urgency amplification: if urgency prototypes appear in top-K, amplify magnitude
/// 5. Clamp to [-1.0, 1.0]
pub fn compute_emotional_valence(prototypes: &[EmotionalPrototype], text_embedding: &[f32]) -> f32 {
    if prototypes.is_empty() || text_embedding.is_empty() {
        return 0.0;
    }

    // Score all prototypes (excluding neutral — they don't carry signal)
    struct ScoredProto {
        sim: f32,
        valence: f32,
        confidence: f32,
        is_urgency: bool,
    }

    // Score all prototypes (positive, negative, urgency, neutral).
    // Neutral prototypes (valence=0.0) participate in the top-K vote,
    // pulling the weighted average toward zero for routine text.
    let mut scored: Vec<ScoredProto> = prototypes
        .iter()
        .filter_map(|p| {
            let sim = cosine_similarity(text_embedding, &p.embedding);
            if sim < VALENCE_MIN_SIMILARITY {
                return None;
            }
            Some(ScoredProto {
                sim,
                valence: p.valence,
                confidence: p.confidence,
                is_urgency: p.category == "urgency",
            })
        })
        .collect();

    if scored.is_empty() {
        return 0.0;
    }

    // Sort by similarity descending, take top-K non-urgency for valence vote
    scored.sort_by(|a, b| {
        b.sim
            .partial_cmp(&a.sim)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut weighted_sum = 0.0f32;
    let mut weight_total = 0.0f32;
    let mut urgency_max_sim = 0.0f32;
    let mut valence_count = 0usize;

    for s in &scored {
        if s.is_urgency {
            if s.sim > urgency_max_sim {
                urgency_max_sim = s.sim;
            }
            continue;
        }
        if valence_count >= VALENCE_TOP_K {
            continue;
        }
        // Cubic weighting: closest prototypes dominate strongly
        let w = s.sim * s.sim * s.sim * s.confidence;
        weighted_sum += w * s.valence;
        weight_total += w;
        valence_count += 1;
    }

    if weight_total == 0.0 {
        return 0.0;
    }

    let mut valence = weighted_sum / weight_total;

    // Urgency amplification: scale by how close text is to urgency prototypes
    if urgency_max_sim > VALENCE_MIN_SIMILARITY && valence.abs() > 0.0 {
        let amplification = URGENCY_AMPLIFICATION * urgency_max_sim;
        valence += valence.signum() * amplification;
    }

    valence.clamp(-1.0, 1.0)
}

/// Legacy keyword-based valence computation. Kept as fallback for tests
/// that validate keyword behavior specifically.
pub fn compute_emotional_valence_keywords(text: &str, kw: &KeywordCache) -> f32 {
    let lowered = text.to_lowercase();
    let mut valence: f32 = 0.0;

    for (keyword, weight) in &kw.negative_valence {
        if lowered.contains(keyword.as_str()) {
            valence += weight;
        }
    }

    for (keyword, weight) in &kw.positive_valence {
        if lowered.contains(keyword.as_str()) {
            valence += weight;
        }
    }

    let urgency_hits = kw
        .urgency
        .iter()
        .filter(|k| lowered.contains(k.as_str()))
        .count();
    if urgency_hits > 0 && valence.abs() > 0.0 {
        let amplification = 0.15 * urgency_hits as f32;
        valence += valence.signum() * amplification;
    }

    valence.clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kw() -> KeywordCache {
        KeywordCache::default_from_static()
    }

    fn prototypes() -> Vec<EmotionalPrototype> {
        seed_emotional_prototypes()
    }

    fn embed(text: &str) -> Vec<f32> {
        super::super::entorhinal::embed_text(text, 384)
    }

    // --- Legacy keyword tests (kept for backward compat) ---

    #[test]
    fn test_negative_valence_bug_report() {
        let v = compute_emotional_valence_keywords("BUG: server crashes on null input", &kw());
        assert!(v < -0.3, "bug+crash should be strongly negative, got {}", v);
    }

    #[test]
    fn test_negative_valence_security() {
        let v = compute_emotional_valence_keywords(
            "Found a SQL injection vulnerability in login",
            &kw(),
        );
        assert!(v < -0.3, "security issue should be negative, got {}", v);
    }

    #[test]
    fn test_negative_valence_data_loss() {
        let v = compute_emotional_valence_keywords(
            "data loss in production after failed migration",
            &kw(),
        );
        assert!(v < -0.5, "data loss should be strongly negative, got {}", v);
    }

    #[test]
    fn test_positive_valence_shipped() {
        let v = compute_emotional_valence_keywords("SHIPPED: v2.0 released successfully", &kw());
        assert!(v > 0.3, "shipping should be positive, got {}", v);
    }

    #[test]
    fn test_positive_valence_fixed() {
        let v = compute_emotional_valence_keywords(
            "Fixed the authentication bug, all tests pass",
            &kw(),
        );
        assert!(v > 0.0, "fix+passing should be positive, got {}", v);
    }

    #[test]
    fn test_neutral_text() {
        let v = compute_emotional_valence_keywords("updated the documentation formatting", &kw());
        assert!(v.abs() < 0.3, "neutral text should be near zero, got {}", v);
    }

    #[test]
    fn test_urgency_amplifies_negative() {
        let without = compute_emotional_valence_keywords("there is a bug in the system", &kw());
        let with = compute_emotional_valence_keywords("BLOCKER: critical bug in the system", &kw());
        assert!(
            with < without,
            "urgency should amplify negative: {} vs {}",
            with,
            without
        );
    }

    #[test]
    fn test_urgency_amplifies_positive() {
        let without = compute_emotional_valence_keywords("shipped the update", &kw());
        let with = compute_emotional_valence_keywords("urgent shipped the update", &kw());
        assert!(
            with > without,
            "urgency should amplify positive: with={} vs without={}",
            with,
            without
        );
    }

    #[test]
    fn test_valence_clamped() {
        let v = compute_emotional_valence_keywords(
            "BLOCKER critical P0 urgent crash panic segfault fatal data loss corruption vulnerability exploit incident outage",
            &kw(),
        );
        assert!(v >= -1.0 && v <= 1.0, "should be clamped, got {}", v);
    }

    #[test]
    fn test_mixed_valence() {
        let v = compute_emotional_valence_keywords("Fixed the crash", &kw());
        let pure_neg = compute_emotional_valence_keywords("the server crashed", &kw());
        assert!(
            v > pure_neg,
            "mixed signals should be less negative than pure negative: {} vs {}",
            v,
            pure_neg
        );
    }

    #[test]
    fn test_empty_text() {
        let v = compute_emotional_valence_keywords("", &kw());
        assert_eq!(v, 0.0);
    }

    // --- New embedding-based prototype tests ---

    #[test]
    fn test_prototype_seeding() {
        let protos = prototypes();
        assert_eq!(protos.len(), SEED_PROTOTYPES.len());
        assert!(protos.len() >= 100);
        // Check embedding dimensions
        for p in &protos {
            assert_eq!(p.embedding.len(), 384);
            assert_eq!(p.confidence, 1.0);
        }
    }

    #[test]
    fn test_prototype_empty_embedding() {
        let v = compute_emotional_valence(&prototypes(), &[]);
        assert_eq!(v, 0.0);
    }

    #[test]
    fn test_prototype_empty_prototypes() {
        let emb = embed("test text");
        let v = compute_emotional_valence(&[], &emb);
        assert_eq!(v, 0.0);
    }

    #[test]
    fn test_prototype_domain_independent_negative() {
        let protos = prototypes();
        let emb = embed("my dog died yesterday");
        let v = compute_emotional_valence(&protos, &emb);
        assert!(
            v < -0.1,
            "domain-independent negative should register, got {}",
            v
        );
    }

    #[test]
    fn test_prototype_domain_independent_positive() {
        let protos = prototypes();
        let emb = embed("got accepted to the PhD program");
        let v = compute_emotional_valence(&protos, &emb);
        assert!(
            v > 0.1,
            "domain-independent positive should register, got {}",
            v
        );
    }

    #[test]
    fn test_prototype_neutral_vs_emotional() {
        let protos = prototypes();
        let neutral_emb = embed("moved the file to another directory");
        let negative_emb = embed("this is a complete disaster, everything is ruined");
        let v_neutral = compute_emotional_valence(&protos, &neutral_emb);
        let v_negative = compute_emotional_valence(&protos, &negative_emb);
        // Neutral text should be much less extreme than strongly emotional text
        assert!(
            v_neutral.abs() < v_negative.abs(),
            "neutral should be less extreme than negative: {} vs {}",
            v_neutral,
            v_negative
        );
    }

    #[test]
    fn test_prototype_negative_vs_positive_direction() {
        let protos = prototypes();
        let neg_emb = embed("we lost all the data and everything is destroyed");
        let pos_emb = embed("we achieved a major breakthrough and everything works perfectly");
        let v_neg = compute_emotional_valence(&protos, &neg_emb);
        let v_pos = compute_emotional_valence(&protos, &pos_emb);
        assert!(
            v_neg < 0.0,
            "loss/destruction should be negative, got {}",
            v_neg
        );
        assert!(
            v_pos > 0.0,
            "breakthrough/success should be positive, got {}",
            v_pos
        );
    }

    #[test]
    fn test_prototype_urgency_preserves_direction() {
        let protos = prototypes();
        // Adding urgency language shifts the embedding but should preserve negative direction
        let emb =
            embed("critical production outage, everything is down and needs immediate action");
        let v = compute_emotional_valence(&protos, &emb);
        assert!(
            v < 0.0,
            "urgency + negative context should stay negative, got {}",
            v
        );
    }

    #[test]
    fn test_prototype_fixed_crash_less_negative() {
        let protos = prototypes();
        let crash_emb = embed("the system crashed and corrupted all the data");
        let fixed_emb = embed("Fixed the crash and everything works now");
        let v_crash = compute_emotional_valence(&protos, &crash_emb);
        let v_fixed = compute_emotional_valence(&protos, &fixed_emb);
        // Pure crash should be more negative than "fixed the crash"
        assert!(
            v_fixed > v_crash,
            "fixed crash should be less negative than pure crash: {} vs {}",
            v_fixed,
            v_crash
        );
    }
}
