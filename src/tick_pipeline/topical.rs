//! Topical-similarity retrieval seed.
//!
//! Given an input embedding, find the substrate's most cosine-close
//! `Signal` elements. The caller (`hebbian_and_salience`) walks the
//! returned ids' relations into the focus set so the frame surfaces
//! topically related context even when the input doesn't mention
//! those entities by name.
//!
//! Without this, retrieval is name-bound — a query like "what was
//! the first issue with my new car?" finds nothing if the haystack
//! talked about a "GPS system" without using the word "issue" in
//! the surrounding text. With this, the GPS-system relation gets
//! pulled in because its embedding is close to the query's.
//!
//! Brute force at v0: score every Signal element. At N ≤ 10K and
//! 384-dim embeddings, this is ~3M float ops — sub-millisecond on
//! any modern CPU. An HNSW/IVF index becomes worth the dependency
//! only past N ≈ 100K.

use std::cmp::Ordering;

use crate::math::dot;
use crate::types::{ElementId, Hypergraph, Polarity};

/// Return the top-`k` Signal elements whose inline embedding is
/// closest (by cosine, which equals dot for L2-normalized vectors)
/// to `embedding`. Skips:
/// - elements with mismatched embedding length (defensive),
/// - elements with empty embeddings (anchor sentinels under default
///   construction),
/// - `Polarity::Void` elements (closed-class grammatical fillers —
///   not topical content).
///
/// Returned ids are in score-descending order, deduped, with stable
/// tie-break on lower `ElementId.0` first.
pub fn topical_neighbors(hg: &Hypergraph, embedding: &[f32], k: usize) -> Vec<ElementId> {
    if k == 0 || embedding.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(ElementId, f32)> = Vec::with_capacity(hg.elements.len());
    for el in &hg.elements {
        if el.polarity == Polarity::Void {
            continue;
        }
        if el.embedding.len() != embedding.len() {
            continue;
        }
        let score = dot(embedding, &el.embedding);
        scored.push((el.id, score));
    }
    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(Ordering::Equal)
            .then(a.0.0.cmp(&b.0.0))
    });
    scored.truncate(k);
    scored.into_iter().map(|(id, _)| id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::EMBEDDING_DIM;
    use crate::seed::{load_seed_graph, rebuild_indices};
    use crate::types::{Element, MemoryStats, Tick};

    fn synth_element(id: u32, name: &str, axis: usize) -> Element {
        let mut emb = vec![0.0f32; EMBEDDING_DIM];
        emb[axis] = 1.0;
        Element {
            id: ElementId(id),
            names: vec![name.to_string()],
            stats: MemoryStats::default(),
            created_at: Tick(0),
            embedding: emb,
            polarity: Polarity::Signal,
        }
    }

    #[test]
    fn returns_empty_for_k_zero() {
        let hg = load_seed_graph();
        let emb = vec![0.5f32; EMBEDDING_DIM];
        assert!(topical_neighbors(&hg, &emb, 0).is_empty());
    }

    #[test]
    fn returns_empty_for_empty_embedding() {
        let hg = load_seed_graph();
        assert!(topical_neighbors(&hg, &[], 5).is_empty());
    }

    #[test]
    fn ranks_by_cosine_descending() {
        // Build a synthetic graph with three orthogonal elements
        // along axes 0, 1, 2. A query along axis 0 should return
        // the axis-0 element first.
        let mut hg = Hypergraph::default();
        hg.elements.push(synth_element(0, "axis0", 0));
        hg.elements.push(synth_element(1, "axis1", 1));
        hg.elements.push(synth_element(2, "axis2", 2));
        rebuild_indices(&mut hg);

        let mut q = vec![0.0f32; EMBEDDING_DIM];
        q[0] = 1.0;
        let neighbors = topical_neighbors(&hg, &q, 3);
        assert_eq!(neighbors, vec![ElementId(0), ElementId(1), ElementId(2)]);
    }

    #[test]
    fn skips_void_elements() {
        let mut hg = Hypergraph::default();
        // Two elements along axis 0: one Signal, one Void. The
        // Void one shouldn't appear in the top-K.
        let mut signal = synth_element(0, "the_signal", 0);
        signal.polarity = Polarity::Signal;
        let mut void = synth_element(1, "the_void", 0);
        void.polarity = Polarity::Void;
        hg.elements.push(signal);
        hg.elements.push(void);
        rebuild_indices(&mut hg);

        let mut q = vec![0.0f32; EMBEDDING_DIM];
        q[0] = 1.0;
        let neighbors = topical_neighbors(&hg, &q, 5);
        assert_eq!(neighbors, vec![ElementId(0)]);
    }

    #[test]
    fn topical_seed_graph_returns_k_elements_with_real_query() {
        // End-to-end smoke test against the actual seed graph + a
        // contextualized query embedding.
        let hg = load_seed_graph();
        let emb = crate::embed::embed_text("calendar appointment scheduling");
        let neighbors = topical_neighbors(&hg, &emb, 10);
        assert_eq!(neighbors.len(), 10);
        // Sanity: ids are unique.
        let mut sorted = neighbors.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 10, "topical_neighbors returned duplicates");
    }
}
