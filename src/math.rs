//! Small numeric helpers used by the embedder, classifiers, and audit code.
//! Pure Rust, no external deps — kept here so they can be shared without
//! pulling extra dependencies into modules that don't need them.

/// Sigmoid: `1 / (1 + e^(-x))`. Maps any real to (0, 1).
pub fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

/// Dot product of two equal-length slices. Debug-asserts equal lengths.
pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len(), "dot: length mismatch");
    a.iter().zip(b.iter()).map(|(&x, &y)| x * y).sum()
}

/// Cosine similarity between two slices that are NOT necessarily unit
/// length. For unit vectors, prefer [`dot`] — same answer, cheaper.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let d = dot(a, b);
    let na = dot(a, a).sqrt();
    let nb = dot(b, b).sqrt();
    d / ((na * nb) + 1e-12)
}
