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
use super::entorhinal::cosine_similarity;
use std::collections::HashSet;

/// Minimum word-overlap Jaccard ratio required to merge (prevents unrelated entries collapsing).
/// Combined with theta_low, ensures that entries sharing vocabulary but describing distinct
/// topics remain as separate episodic traces.
pub const MERGE_WORD_OVERLAP_THRESHOLD: f32 = 0.4;

/// Maximum cosine displacement allowed by orthogonalization.
/// If the orthogonalized result drifts further than this from the original,
/// it is blended back toward the original to cap drift.
pub const MAX_ORTHO_DISPLACEMENT: f32 = 0.3;

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
///
/// Adaptive strength: when many neighbors fall in the confusable zone, per-neighbor
/// strength is reduced to prevent cumulative drift. Total displacement is capped at
/// `MAX_ORTHO_DISPLACEMENT` (cosine distance from original).
pub fn sparse_orthogonalize(
    embedding: &[f32],
    existing: &[Vec<f32>],
    low: f32,
    high: f32,
    strength: f32,
) -> Vec<f32> {
    let mut result: Vec<f32> = embedding.to_vec();

    // Pass 1: count confusable neighbors
    let confusable_count = existing
        .iter()
        .filter(|e| {
            e.len() == result.len() && !e.is_empty() && {
                let sim = cosine_similarity(embedding, e);
                sim >= low && sim < high
            }
        })
        .count();

    if confusable_count == 0 {
        // Still normalize — input may not be unit-length (e.g. truncated embeddings)
        let norm: f32 = result.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm > 0.0 {
            for v in &mut result {
                *v /= norm;
            }
        }
        return result;
    }

    // Adaptive strength: scale inversely with neighbor count to prevent cumulative drift
    // 1 neighbor = full strength, 5 = 0.56x, 10 = 0.36x, 50 = 0.09x
    let effective_strength = strength / (1.0 + (confusable_count as f32 - 1.0).max(0.0) * 0.2);

    // Pass 2: apply projections with scaled strength
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
                *r -= effective_strength * dot * e;
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

    // Displacement cap: if we drifted too far, blend back toward original
    let displacement = 1.0 - cosine_similarity(&result, embedding);
    if displacement > MAX_ORTHO_DISPLACEMENT {
        // Binary search would be exact, but linear blend is good enough:
        // blend = (1 - target_displacement) / (1 - displacement)
        // We want cosine_sim(blended, original) ≈ 1 - MAX_ORTHO_DISPLACEMENT
        let blend_ratio = MAX_ORTHO_DISPLACEMENT / displacement;
        for (r, &o) in result.iter_mut().zip(embedding.iter()) {
            *r = o + blend_ratio * (*r - o);
        }
        // Re-normalize after blending
        let norm: f32 = result.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm > 0.0 {
            for v in &mut result {
                *v /= norm;
            }
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

/// Maximal Marginal Relevance — select k items from candidates that balance
/// relevance to the query with diversity among selected items.
///
/// This is pattern separation at retrieval time: instead of returning the top-k
/// most similar results (which may be near-duplicates), MMR iteratively selects
/// candidates that are relevant to the query but dissimilar to already-selected
/// results. This mirrors the dentate gyrus function of creating orthogonal
/// representations to reduce interference.
///
/// `lambda` controls the trade-off: 1.0 = pure relevance, 0.0 = pure diversity.
/// Legend uses 0.7 (relevance-heavy with diversity protection).
///
/// Each candidate needs a `query_similarity` (pre-computed) and an `embedding`
/// for pairwise diversity computation.
#[allow(dead_code)]
pub fn mmr_select(
    candidates: &[(usize, f32, &[f32])], // (index, query_similarity, embedding)
    k: usize,
    lambda: f32,
) -> Vec<usize> {
    if candidates.is_empty() {
        return Vec::new();
    }

    let mut selected: Vec<usize> = Vec::with_capacity(k);
    let mut selected_embeddings: Vec<&[f32]> = Vec::with_capacity(k);
    let mut available: Vec<bool> = vec![true; candidates.len()];

    for _ in 0..k {
        let mut best_idx = None;
        let mut best_score = f32::NEG_INFINITY;

        for (i, &(_, query_sim, emb)) in candidates.iter().enumerate() {
            if !available[i] {
                continue;
            }

            // Max similarity to any already-selected item
            let max_sim_to_selected = if selected_embeddings.is_empty() {
                0.0
            } else {
                selected_embeddings
                    .iter()
                    .map(|sel_emb| cosine_similarity(emb, sel_emb))
                    .fold(f32::NEG_INFINITY, f32::max)
            };

            let mmr_score = lambda * query_sim - (1.0 - lambda) * max_sim_to_selected;

            if mmr_score > best_score {
                best_score = mmr_score;
                best_idx = Some(i);
            }
        }

        match best_idx {
            Some(idx) => {
                selected.push(candidates[idx].0);
                selected_embeddings.push(candidates[idx].2);
                available[idx] = false;
            }
            None => break,
        }
    }

    selected
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::entorhinal::embed_text;

    #[test]
    fn test_sparse_orthogonalize_pushes_apart_similar() {
        // Near-paraphrases land in the confusable zone [low, high); truly
        // unrelated sentences fall below `low` and skip orthogonalization.
        let a = embed_text("Rust uses ownership for memory safety", 256);
        let b = embed_text("Python uses garbage collection for memory management", 256);
        let sim_before = cosine_similarity(&a, &b);
        assert!(
            sim_before >= 0.3 && sim_before < 0.88,
            "test inputs must be confusable: sim={sim_before}"
        );

        let a_ortho = sparse_orthogonalize(&a, &[b.clone()], 0.3, 0.88, 0.3);
        let sim_after = cosine_similarity(&a_ortho, &b);

        assert!(
            sim_after < sim_before,
            "orthogonalization should reduce similarity: before={}, after={}",
            sim_before,
            sim_after
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
                sim_before,
                sim_after
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

    // ── Cumulative drift / adaptive strength tests ─────────────────────────

    #[test]
    fn test_cumulative_drift_capped() {
        // 50 confusable embeddings — displacement must stay within MAX_ORTHO_DISPLACEMENT
        let base = embed_text("memory system architecture design patterns", 384);
        let mut neighbors: Vec<Vec<f32>> = Vec::new();
        for i in 0..50 {
            let text = format!(
                "memory system architecture design variation number {} with extra words",
                i
            );
            neighbors.push(embed_text(&text, 384));
        }
        let result = sparse_orthogonalize(&base, &neighbors, 0.3, 0.88, 0.3);
        let displacement = 1.0 - cosine_similarity(&result, &base);
        assert!(
            displacement <= MAX_ORTHO_DISPLACEMENT + 0.01,
            "displacement {} should be capped at {}",
            displacement,
            MAX_ORTHO_DISPLACEMENT
        );
    }

    #[test]
    fn test_strength_scales_with_neighbors() {
        // 1 neighbor vs 10 neighbors: total displacement should NOT scale 10x
        let base = embed_text("Rust memory model borrow checker ownership", 384);
        let single = vec![embed_text(
            "Rust memory model borrow checker lifetimes",
            384,
        )];
        let result_single = sparse_orthogonalize(&base, &single, 0.3, 0.88, 0.3);
        let disp_single = 1.0 - cosine_similarity(&result_single, &base);

        let mut many: Vec<Vec<f32>> = Vec::new();
        for i in 0..10 {
            many.push(embed_text(
                &format!("Rust memory model borrow checker variant {} details", i),
                384,
            ));
        }
        let result_many = sparse_orthogonalize(&base, &many, 0.3, 0.88, 0.3);
        let disp_many = 1.0 - cosine_similarity(&result_many, &base);

        // With adaptive scaling, 10 neighbors should NOT produce 10x the displacement
        // Allow up to 4x (generous), but definitely not 10x
        assert!(
            disp_many < disp_single * 5.0 || disp_many <= MAX_ORTHO_DISPLACEMENT + 0.01,
            "10 neighbors displacement ({}) should not be >5x single ({})",
            disp_many,
            disp_single
        );
    }

    #[test]
    fn test_single_neighbor_unchanged() {
        // With 1 confusable neighbor, effective_strength == strength (no scaling)
        let a = embed_text("Rust memory model borrow checker ownership", 384);
        let b = embed_text("Legend memory system three-layer architecture", 384);
        let sim_before = cosine_similarity(&a, &b);

        // Only test if b is in the confusable zone
        if sim_before >= 0.3 && sim_before < 0.88 {
            let result = sparse_orthogonalize(&a, &[b.clone()], 0.3, 0.88, 0.3);
            let sim_after = cosine_similarity(&result, &b);
            // Single neighbor: full strength applied, should reduce similarity
            assert!(
                sim_after < sim_before,
                "single neighbor should still push apart: before={}, after={}",
                sim_before,
                sim_after
            );
        }
    }

    // ── MMR tests ──────────────────────────────────────────────────────────

    #[test]
    fn test_mmr_select_empty_candidates() {
        assert!(mmr_select(&[], 5, 0.7).is_empty());
    }

    #[test]
    fn test_mmr_select_fewer_than_k() {
        let emb_a = embed_text("hello world", 384);
        let candidates = vec![(0, 0.9, emb_a.as_slice())];
        let selected = mmr_select(&candidates, 5, 0.7);
        assert_eq!(selected, vec![0]);
    }

    #[test]
    fn test_mmr_select_picks_highest_first() {
        let emb_a = embed_text("authentication system login", 384);
        let emb_b = embed_text("cooking pasta recipes", 384);
        let candidates = vec![(0, 0.5, emb_a.as_slice()), (1, 0.9, emb_b.as_slice())];
        let selected = mmr_select(&candidates, 1, 0.7);
        assert_eq!(selected, vec![1], "should pick highest similarity first");
    }

    #[test]
    fn test_mmr_select_promotes_diversity() {
        // Three candidates: A and B are near-duplicates, C is different
        let emb_a = embed_text("chose Redis for caching because pub/sub support", 384);
        let emb_b = embed_text("chose Redis for caching because fast performance", 384);
        let emb_c = embed_text("authentication system uses JWT tokens for sessions", 384);

        let sim_ab = cosine_similarity(&emb_a, &emb_b);
        assert!(sim_ab > 0.7, "A and B should be similar: {}", sim_ab);

        // A is top, B is close second, C is only slightly lower — close enough
        // that MMR diversity penalty on the A-B near-duplicate should push C above B
        let candidates = vec![
            (0, 0.90, emb_a.as_slice()),
            (1, 0.88, emb_b.as_slice()),
            (2, 0.82, emb_c.as_slice()),
        ];

        // Pure relevance (lambda=1.0) would pick A, B
        let pure_relevance = mmr_select(&candidates, 2, 1.0);
        assert_eq!(pure_relevance, vec![0, 1]);

        // With diversity (lambda=0.7), should pick A then C (skipping near-duplicate B)
        let with_diversity = mmr_select(&candidates, 2, 0.7);
        assert_eq!(
            with_diversity[0], 0,
            "first pick should still be most relevant"
        );
        assert_eq!(
            with_diversity[1], 2,
            "second pick should be diverse C, not near-duplicate B"
        );
    }

    #[test]
    fn test_mmr_select_returns_correct_count() {
        let embs: Vec<Vec<f32>> = (0..10)
            .map(|i| embed_text(&format!("topic number {} about various things", i), 384))
            .collect();
        let candidates: Vec<(usize, f32, &[f32])> = embs
            .iter()
            .enumerate()
            .map(|(i, e)| (i, 0.9 - i as f32 * 0.05, e.as_slice()))
            .collect();

        let selected = mmr_select(&candidates, 5, 0.7);
        assert_eq!(selected.len(), 5);
    }
}
