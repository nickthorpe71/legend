/// Thalamus — Sensory encoding relay hub.
///
/// The thalamus is the brain's central relay station: nearly all sensory input
/// passes through it before reaching the cortex.  It doesn't interpret signals
/// itself but transforms them into a representation the rest of the brain can
/// use for matching, gating, and routing.
///
/// In Legend, `thalamus.rs` fills exactly that role:
///
/// - **N-gram hashing as perceptual representation** — raw text is transformed
///   into a 256-dimensional embedding via FNV-1a hashing of word unigrams,
///   character trigrams, and word bigrams.  This is the "sensory encoding" step:
///   converting arbitrary input into a fixed-size vector the downstream modules
///   (hippocampus, neocortex, dentate gyrus) can compare and manipulate.
///
/// - **Cosine similarity as neural distance** — the distance between two
///   embeddings is measured by cosine similarity, analogous to the neural
///   "closeness" of two percepts.  High similarity → same memory trace;
///   moderate → related but distinct; low → novel input.
///
/// - **Salience scoring as attentional gating** — `compute_salience()` assigns
///   an importance prior to each incoming chunk based on keyword heuristics
///   (decisions, bugs, architecture, preferences, code definitions).  This
///   determines whether the prefrontal attention gate promotes the input to
///   episodic memory (L2) or lets it fade in working memory (L1).
///
/// The thalamus is deliberately stateless: it transforms input and returns
/// values without mutating `BrainState`.  All side effects happen in the
/// calling orchestrator (`tick_impl`) and downstream brain-region modules.
use crate::memory::wernicke::KeywordCache;

/// Compute an n-gram embedding vector of given dimension.
///
/// Uses word unigrams, character trigrams, and word bigrams,
/// then L2-normalizes for cosine similarity.
pub fn embed_text(text: &str, dim: usize) -> Vec<f32> {
    let mut vector = vec![0.0f32; dim];
    let lowered = text.to_lowercase();
    let tokens: Vec<&str> = lowered.split_whitespace().collect();

    // Word unigrams
    for token in &tokens {
        let idx = (fnv_hash(token.as_bytes()) as usize) % dim;
        vector[idx] += 1.0;
    }

    // Character trigrams (captures subword similarity)
    for token in &tokens {
        let chars: Vec<char> = token.chars().collect();
        if chars.len() >= 3 {
            for window in chars.windows(3) {
                let trigram: String = window.iter().collect();
                let idx = (fnv_hash(trigram.as_bytes()) as usize) % dim;
                vector[idx] += 0.3;
            }
        }
    }

    // Word bigrams (captures phrase structure)
    for pair in tokens.windows(2) {
        let bigram = format!("{} {}", pair[0], pair[1]);
        let idx = (fnv_hash(bigram.as_bytes()) as usize) % dim;
        vector[idx] += 0.75;
    }

    // L2-normalize
    let norm: f32 = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 0.0 {
        for v in &mut vector {
            *v /= norm;
        }
    }

    vector
}

/// FNV-1a 64-bit hash.
pub fn fnv_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Cosine similarity between two vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0;
    let mut norm_a = 0.0;
    let mut norm_b = 0.0;

    for i in 0..a.len().min(b.len()) {
        dot += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    dot / (norm_a.sqrt() * norm_b.sqrt())
}

/// Element-wise average of two embedding vectors.
pub fn merge_embeddings(a: &[f32], b: &[f32]) -> Vec<f32> {
    let len = a.len().min(b.len());
    (0..len).map(|i| (a[i] + b[i]) / 2.0).collect()
}

/// Compute salience score from text content heuristics.
pub fn compute_salience(text: &str, kw: &KeywordCache) -> f32 {
    let mut score: f32 = 0.0;
    let lowered = text.to_lowercase();

    // Decision language — highest importance
    let decision_hits = kw.decision
        .iter()
        .filter(|k| lowered.contains(k.as_str()))
        .count();
    if decision_hits >= 2 {
        score += 0.5;
    } else if decision_hits >= 1 {
        score += 0.3;
    }
    // Rationale language amplifies decisions
    if decision_hits > 0
        && (lowered.contains("because")
            || lowered.contains("rationale")
            || lowered.contains("reason"))
    {
        score += 0.15;
    }

    // Bug/incident language — high importance
    if kw.bug.iter().any(|k| lowered.contains(k.as_str())) {
        score += 0.4;
    }

    // TODO/blocker language
    if kw.todo.iter().any(|k| lowered.contains(k.as_str())) {
        score += 0.3;
    }

    // Architecture/structural statements
    if kw.architecture.iter().any(|k| lowered.contains(k.as_str())) {
        score += 0.25;
    }

    // Preference/convention
    if kw.preference.iter().any(|k| lowered.contains(k.as_str())) {
        score += 0.3;
    }

    // Domain-specific vocabulary (learned from workspace)
    if !kw.domain.is_empty() && kw.domain.iter().any(|k| lowered.contains(k.as_str())) {
        score += 0.1;
    }

    // Code references — distinguish definitions from mere mentions
    if text.contains("```") {
        score += 0.15;
    }
    let code_def_hits = kw.code
        .iter()
        .filter(|(trigger, _, _, _)| lowered.contains(trigger.as_str()))
        .count();
    if code_def_hits >= 2 {
        // Multiple code definitions (e.g. "fn foo uses struct Bar") — high signal
        score += 0.3;
    } else if code_def_hits == 1 {
        score += 0.2;
    }

    // Substantive text (not too short)
    let word_count = text.split_whitespace().count();
    if word_count > 25 {
        score += 0.15;
    } else if word_count > 50 {
        score += 0.2;
    }

    // Error mentions
    if lowered.contains("error") {
        score += 0.15;
    }

    score.clamp(0.05, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embed_produces_correct_dimensions() {
        assert_eq!(embed_text("hello world", 256).len(), 256);
    }

    #[test]
    fn test_embed_is_normalized() {
        let vec = embed_text("the quick brown fox jumps over the lazy dog", 256);
        let norm: f32 = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.01, "norm={}", norm);
    }

    #[test]
    fn test_cosine_similarity_identical() {
        let a = embed_text("hello world", 256);
        let sim = cosine_similarity(&a, &a);
        assert!((sim - 1.0).abs() < 0.001, "got {}", sim);
    }

    #[test]
    fn test_cosine_similarity_related_vs_unrelated() {
        let a = embed_text("memory system with embeddings and similarity search", 256);
        let b = embed_text("embedding vectors and cosine similarity matching", 256);
        let c = embed_text("cooking recipes for italian pasta dishes", 256);
        assert!(cosine_similarity(&a, &b) > cosine_similarity(&a, &c));
    }

    #[test]
    fn test_cosine_similarity_empty() {
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
    }

    #[test]
    fn test_merge_embeddings() {
        assert_eq!(
            merge_embeddings(&[1.0, 0.0, 0.5], &[0.0, 1.0, 0.5]),
            vec![0.5, 0.5, 0.5]
        );
    }

    #[test]
    fn test_fnv_hash_deterministic() {
        let h1 = fnv_hash(b"hello");
        assert_eq!(h1, fnv_hash(b"hello"));
        assert_ne!(h1, fnv_hash(b"world"));
    }

    fn kw() -> KeywordCache {
        KeywordCache::default_from_static()
    }

    #[test]
    fn test_compute_salience_code() {
        assert!(compute_salience("fn main() { struct Foo {} }", &kw()) > compute_salience("regular text", &kw()));
    }

    #[test]
    fn test_compute_salience_todo() {
        assert!(compute_salience("TODO: fix bug", &kw()) > compute_salience("no urgency here", &kw()));
    }

    #[test]
    fn test_compute_salience_decision_language() {
        let s = compute_salience("DECISION: Chose Tokio over async-std because broader ecosystem", &kw());
        assert!(s >= 0.3, "decision text should score high, got {}", s);
    }

    #[test]
    fn test_compute_salience_decision_with_rationale_boost() {
        let without = compute_salience("Decided to use Redis", &kw());
        let with = compute_salience("Decided to use Redis because it has better pub/sub", &kw());
        assert!(with > without, "rationale should boost: {} vs {}", with, without);
    }

    #[test]
    fn test_compute_salience_bug_language() {
        let s = compute_salience("Bug: the server crashes on empty input", &kw());
        assert!(s >= 0.4, "bug text should score high, got {}", s);
    }

    #[test]
    fn test_compute_salience_blocker() {
        let s = compute_salience("BLOCKER: blocked on API key provisioning", &kw());
        assert!(s >= 0.3, "blocker text should score high, got {}", s);
    }

    #[test]
    fn test_compute_salience_architecture() {
        let s = compute_salience("The API layer interfaces with the schema module", &kw());
        assert!(s >= 0.25, "architecture text should score, got {}", s);
    }

    #[test]
    fn test_compute_salience_preference() {
        let s = compute_salience("User prefers dark mode, always use minimal UI", &kw());
        assert!(s >= 0.3, "preference text should score, got {}", s);
    }

    #[test]
    fn test_compute_salience_minimum_floor() {
        let s = compute_salience("nothing special here at all", &kw());
        assert!(s >= 0.05, "floor should be 0.05, got {}", s);
    }

    #[test]
    fn test_compute_salience_capped_at_one() {
        // Pile on every keyword category
        let s = compute_salience("DECISION: Chose X because crash bug regression BLOCKER TODO architecture API schema module user prefers convention fn main() {} ``` code ```", &kw());
        assert!(s <= 1.0, "should cap at 1.0, got {}", s);
    }

    #[test]
    fn test_embed_empty_input() {
        let v = embed_text("", 256);
        assert_eq!(v.len(), 256);
        // Empty input should still produce a vector (all zeros is fine)
    }

    #[test]
    fn test_embed_single_word() {
        let v = embed_text("hello", 256);
        assert_eq!(v.len(), 256);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.01 || norm == 0.0);
    }

    #[test]
    fn test_cosine_similarity_different_lengths() {
        // Should handle gracefully (min length)
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!((0.0..=1.0).contains(&sim), "got {}", sim);
    }

    #[test]
    fn test_merge_embeddings_different_lengths() {
        let result = merge_embeddings(&[1.0, 2.0, 3.0], &[4.0, 5.0]);
        assert_eq!(result.len(), 2); // min length
        assert_eq!(result, vec![2.5, 3.5]);
    }
}
