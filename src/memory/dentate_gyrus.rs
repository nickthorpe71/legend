/// Dentate Gyrus — Pattern Separation Module
///
/// The dentate gyrus is the hippocampal subregion responsible for pattern separation:
/// transforming similar cortical inputs into sparse, orthogonal representations so that
/// related-but-distinct memories don't interfere with each other during storage and retrieval.
///
/// This module provides two complementary mechanisms:
///
/// 1. **Sparse orthogonalization** — at encoding time, new embeddings are pushed away from
///    existing similar-but-distinct L2 embeddings, creating sparser representations that
///    reduce retrieval interference.
///
/// 2. **Diversity gating** — at merge time, word-overlap Jaccard similarity provides a
///    second check beyond cosine similarity. Two entries must share enough actual vocabulary
///    (not just hash-bucket overlap) to be considered the "same" memory.
use super::thalamus::cosine_similarity;
use std::collections::HashSet;

/// Minimum word-overlap Jaccard ratio required to merge (prevents unrelated entries collapsing).
/// Combined with theta_low, ensures that entries sharing vocabulary but describing distinct
/// topics remain as separate episodic traces.
pub const MERGE_WORD_OVERLAP_THRESHOLD: f32 = 0.4;

/// Dentate Gyrus sparse orthogonalization.
///
/// Transforms a new embedding to be more orthogonal to similar-but-distinct existing
/// embeddings. This mimics how the dentate gyrus creates sparse, non-overlapping
/// representations from dense cortical input — reducing interference between
/// related-but-different memories.
///
/// For each existing embedding in the "similar but distinct" zone (similarity between
/// `low` and `high`), subtracts a scaled projection of the existing embedding from
/// the new one. The result is re-normalized. Embeddings that are very similar (above
/// `high`) or dissimilar (below `low`) are left alone — only the confusable middle
/// range gets orthogonalized.
pub fn sparse_orthogonalize(
    embedding: &[f32],
    existing: &[Vec<f32>],
    low: f32,
    high: f32,
    strength: f32,
) -> Vec<f32> {
    let mut result: Vec<f32> = embedding.to_vec();

    for existing_emb in existing {
        if existing_emb.len() != result.len() || existing_emb.is_empty() {
            continue;
        }
        let sim = cosine_similarity(&result, existing_emb);
        if sim >= low && sim < high {
            // Subtract the projection of existing onto result, scaled by strength
            // projection = (result · existing) * existing
            let dot: f32 = result
                .iter()
                .zip(existing_emb.iter())
                .map(|(a, b)| a * b)
                .sum();
            for (r, e) in result.iter_mut().zip(existing_emb.iter()) {
                *r -= strength * dot * e;
            }
        }
    }

    // Re-normalize to unit length
    let norm: f32 = result.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 0.0 {
        for v in &mut result {
            *v /= norm;
        }
    }

    result
}

/// Jaccard word overlap between two texts.
///
/// Tokenizes both strings, strips punctuation, filters short words, and returns
/// |intersection| / |union|. Used as the diversity gate: entries must share enough
/// actual vocabulary to be considered the same memory trace.
pub fn word_overlap(a: &str, b: &str) -> f32 {
    let set_a: HashSet<&str> = a
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|w| w.len() > 1)
        .collect();
    let set_b: HashSet<&str> = b
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|w| w.len() > 1)
        .collect();
    if set_a.is_empty() || set_b.is_empty() {
        return 0.0;
    }
    let intersection = set_a.intersection(&set_b).count() as f32;
    let union = set_a.union(&set_b).count() as f32;
    intersection / union
}

/// Diversity gate: returns true if two texts share enough vocabulary to be considered
/// the same memory trace (and thus eligible for merging).
pub fn diversity_pass(existing_text: &str, new_text: &str) -> bool {
    word_overlap(existing_text, new_text) >= MERGE_WORD_OVERLAP_THRESHOLD
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::thalamus::embed_text;

    #[test]
    fn test_sparse_orthogonalize_pushes_apart_similar() {
        let a = embed_text("Rust memory model borrow checker ownership", 256);
        let b = embed_text("Legend memory system three-layer architecture", 256);
        let sim_before = cosine_similarity(&a, &b);

        let a_ortho = sparse_orthogonalize(&a, &[b.clone()], 0.3, 0.88, 0.3);
        let sim_after = cosine_similarity(&a_ortho, &b);

        assert!(
            sim_after < sim_before,
            "orthogonalization should reduce similarity: before={}, after={}",
            sim_before, sim_after
        );
    }

    #[test]
    fn test_sparse_orthogonalize_preserves_unit_norm() {
        let a = embed_text("memory system embeddings", 256);
        let b = embed_text("memory architecture design", 256);
        let result = sparse_orthogonalize(&a, &[b], 0.3, 0.88, 0.3);

        let norm: f32 = result.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!(
            (norm - 1.0).abs() < 0.01,
            "result should be unit-normalized, got norm={}",
            norm
        );
    }

    #[test]
    fn test_sparse_orthogonalize_ignores_dissimilar() {
        let a = embed_text("Rust borrow checker ownership semantics", 256);
        let b = embed_text("Italian pasta cooking recipes ingredients", 256);
        let result = sparse_orthogonalize(&a, &[b.clone()], 0.3, 0.88, 0.3);

        let sim_original = cosine_similarity(&a, &result);
        assert!(
            sim_original > 0.99,
            "dissimilar embeddings should not be orthogonalized: sim={}",
            sim_original
        );
    }

    #[test]
    fn test_sparse_orthogonalize_ignores_near_identical() {
        let a = embed_text("chose Redis for caching because pub/sub support", 256);
        let b = embed_text("chose Redis for caching because pub/sub integration", 256);
        let sim_before = cosine_similarity(&a, &b);

        let a_ortho = sparse_orthogonalize(&a, &[b.clone()], 0.3, 0.88, 0.3);
        let sim_after = cosine_similarity(&a_ortho, &b);

        if sim_before >= 0.88 {
            assert!(
                (sim_after - sim_before).abs() < 0.05,
                "near-identical should be unchanged: before={}, after={}",
                sim_before, sim_after
            );
        }
    }

    #[test]
    fn test_sparse_orthogonalize_empty_existing() {
        let a = embed_text("some text", 256);
        let result = sparse_orthogonalize(&a, &[], 0.3, 0.88, 0.3);
        assert_eq!(result.len(), a.len());
        let sim = cosine_similarity(&a, &result);
        assert!((sim - 1.0).abs() < 0.001, "no existing → no change");
    }

    #[test]
    fn test_word_overlap_identical() {
        assert!((word_overlap("hello world", "hello world") - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_word_overlap_disjoint() {
        assert!(word_overlap("apples oranges bananas", "cars trucks bikes") < 0.01);
    }

    #[test]
    fn test_diversity_pass_similar_vocabulary() {
        assert!(diversity_pass(
            "chose Redis for caching pub/sub",
            "chose Redis for caching integration"
        ));
    }

    #[test]
    fn test_diversity_pass_rejects_different_topics() {
        assert!(!diversity_pass(
            "Rust memory model borrow checker",
            "cooking recipes fresh ingredients"
        ));
    }
}
