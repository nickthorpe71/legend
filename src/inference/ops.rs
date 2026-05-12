//! Low-level math kernels for the BERT forward pass.
//!
//! Matrix multiplication delegates to the `matrixmultiply` crate
//! (pure-Rust SIMD `sgemm`). LayerNorm / GELU / softmax / pooling are
//! tight scalar loops that auto-vectorize well under `opt-level=3`.

// Same Rust 2024 friction as in `quantized_ops.rs`: explicit unsafe
// inside unsafe fns gets flagged as `unused_unsafe`. We keep the
// blocks for readability and silence the lint.
#![allow(unused_unsafe)]

/// Compute C = α·A·B + β·C with row-major fp32 matrices.
///
/// A is [m, k], B is [k, n], C is [m, n], all row-major.
/// Uses `matrixmultiply::sgemm` under the hood — pure-Rust SIMD,
/// no BLAS dependency.
pub fn matmul(m: usize, k: usize, n: usize, a: &[f32], b: &[f32], c: &mut [f32]) {
    debug_assert_eq!(a.len(), m * k);
    debug_assert_eq!(b.len(), k * n);
    debug_assert_eq!(c.len(), m * n);
    unsafe {
        matrixmultiply::sgemm(
            m,
            k,
            n,
            1.0,
            a.as_ptr(),
            k as isize,
            1,
            b.as_ptr(),
            n as isize,
            1,
            0.0,
            c.as_mut_ptr(),
            n as isize,
            1,
        );
    }
}

/// Compute C = A · B^T (B is naturally [n, k], so accessing it as if
/// it were [k, n] requires the strides to be swapped). Same row-major
/// fp32 layout otherwise. Useful for attention scores: Q · K^T.
pub fn matmul_at_bt(m: usize, k: usize, n: usize, a: &[f32], b_t: &[f32], c: &mut [f32]) {
    debug_assert_eq!(a.len(), m * k);
    debug_assert_eq!(b_t.len(), n * k);
    debug_assert_eq!(c.len(), m * n);
    unsafe {
        matrixmultiply::sgemm(
            m,
            k,
            n,
            1.0,
            a.as_ptr(),
            k as isize,
            1,
            // b_t is [n, k] row-major; treat as [k, n] with swapped strides.
            b_t.as_ptr(),
            1,
            k as isize,
            0.0,
            c.as_mut_ptr(),
            n as isize,
            1,
        );
    }
}

/// In-place bias add along the last axis. `bias` has length `cols`.
/// `x` has length `rows * cols`.
pub fn add_bias_inplace(x: &mut [f32], bias: &[f32], rows: usize, cols: usize) {
    debug_assert_eq!(x.len(), rows * cols);
    debug_assert_eq!(bias.len(), cols);
    for r in 0..rows {
        let row = &mut x[r * cols..(r + 1) * cols];
        for (xi, &bi) in row.iter_mut().zip(bias.iter()) {
            *xi += bi;
        }
    }
}

/// In-place residual add: `x += y`. Lengths must match.
pub fn add_inplace(x: &mut [f32], y: &[f32]) {
    debug_assert_eq!(x.len(), y.len());
    for (xi, &yi) in x.iter_mut().zip(y.iter()) {
        *xi += yi;
    }
}

/// LayerNorm applied per row of an `[rows, cols]` tensor. Each row is
/// normalized to mean 0 / variance 1, then scaled by `gamma` and
/// shifted by `beta` (per-dim parameters of length `cols`).
///
/// Uses the BERT convention: variance is population (divide by N),
/// not sample (divide by N-1).
pub fn layernorm_inplace(
    x: &mut [f32],
    rows: usize,
    cols: usize,
    gamma: &[f32],
    beta: &[f32],
    eps: f32,
) {
    debug_assert_eq!(x.len(), rows * cols);
    debug_assert_eq!(gamma.len(), cols);
    debug_assert_eq!(beta.len(), cols);

    #[cfg(target_arch = "x86_64")]
    {
        if is_avx2_fma_available() {
            for r in 0..rows {
                let row = &mut x[r * cols..(r + 1) * cols];
                unsafe { layernorm_row_avx2(row, gamma, beta, eps) };
            }
            return;
        }
    }
    layernorm_inplace_scalar(x, rows, cols, gamma, beta, eps);
}

fn layernorm_inplace_scalar(
    x: &mut [f32],
    rows: usize,
    cols: usize,
    gamma: &[f32],
    beta: &[f32],
    eps: f32,
) {
    let inv_cols = 1.0 / cols as f32;
    for r in 0..rows {
        let row = &mut x[r * cols..(r + 1) * cols];
        let mut sum = 0.0f32;
        for &v in row.iter() {
            sum += v;
        }
        let mean = sum * inv_cols;
        let mut var_sum = 0.0f32;
        for &v in row.iter() {
            let d = v - mean;
            var_sum += d * d;
        }
        let inv_std = 1.0 / (var_sum * inv_cols + eps).sqrt();
        for (i, v) in row.iter_mut().enumerate() {
            *v = (*v - mean) * inv_std * gamma[i] + beta[i];
        }
    }
}

/// AVX2+FMA per-row layernorm. Two passes (sum, var-sum) over 8-wide
/// SIMD then a third pass to write back. Mathematically equivalent
/// to the scalar two-pass formulation.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "fma")]
unsafe fn layernorm_row_avx2(row: &mut [f32], gamma: &[f32], beta: &[f32], eps: f32) {
    use std::arch::x86_64::*;
    let cols = row.len();
    debug_assert_eq!(gamma.len(), cols);
    debug_assert_eq!(beta.len(), cols);
    let inv_cols = 1.0 / cols as f32;
    let chunks = cols / 8;
    let tail_start = chunks * 8;

    // ── Pass 1: sum ────────────────────────────────────────────────
    let mut sum_v = unsafe { _mm256_setzero_ps() };
    for c in 0..chunks {
        let v = unsafe { _mm256_loadu_ps(row.as_ptr().add(c * 8)) };
        sum_v = unsafe { _mm256_add_ps(sum_v, v) };
    }
    let mut tmp = [0.0f32; 8];
    unsafe { _mm256_storeu_ps(tmp.as_mut_ptr(), sum_v) };
    let sum = tmp.iter().sum::<f32>() + row[tail_start..].iter().sum::<f32>();
    let mean = sum * inv_cols;

    // ── Pass 2: var sum ────────────────────────────────────────────
    let mean_v = unsafe { _mm256_set1_ps(mean) };
    let mut var_v = unsafe { _mm256_setzero_ps() };
    for c in 0..chunks {
        let v = unsafe { _mm256_loadu_ps(row.as_ptr().add(c * 8)) };
        let d = unsafe { _mm256_sub_ps(v, mean_v) };
        var_v = unsafe { _mm256_fmadd_ps(d, d, var_v) };
    }
    unsafe { _mm256_storeu_ps(tmp.as_mut_ptr(), var_v) };
    let mut var_sum = tmp.iter().sum::<f32>();
    for &v in &row[tail_start..] {
        let d = v - mean;
        var_sum += d * d;
    }
    let inv_std = 1.0 / (var_sum * inv_cols + eps).sqrt();

    // ── Pass 3: write back ─────────────────────────────────────────
    let inv_std_v = unsafe { _mm256_set1_ps(inv_std) };
    for c in 0..chunks {
        let v = unsafe { _mm256_loadu_ps(row.as_ptr().add(c * 8)) };
        let g = unsafe { _mm256_loadu_ps(gamma.as_ptr().add(c * 8)) };
        let b = unsafe { _mm256_loadu_ps(beta.as_ptr().add(c * 8)) };
        let d = unsafe { _mm256_sub_ps(v, mean_v) };
        let scaled = unsafe { _mm256_mul_ps(_mm256_mul_ps(d, inv_std_v), g) };
        let out = unsafe { _mm256_add_ps(scaled, b) };
        unsafe { _mm256_storeu_ps(row.as_mut_ptr().add(c * 8), out) };
    }
    for kk in tail_start..cols {
        row[kk] = (row[kk] - mean) * inv_std * gamma[kk] + beta[kk];
    }
}

/// Exact GELU (the original BERT formulation, not the tanh
/// approximation). `gelu(x) = 0.5 · x · (1 + erf(x / √2))`.
/// In-place: each element of `x` is replaced by `gelu(x)`.
pub fn gelu_inplace(x: &mut [f32]) {
    #[cfg(target_arch = "x86_64")]
    {
        if is_avx2_fma_available() {
            unsafe { gelu_inplace_avx2(x) };
            return;
        }
    }
    gelu_inplace_scalar(x);
}

fn gelu_inplace_scalar(x: &mut [f32]) {
    // 1/sqrt(2) precomputed.
    const INV_SQRT_2: f32 = 0.707_106_77_f32;
    for v in x.iter_mut() {
        *v = 0.5 * *v * (1.0 + erf(*v * INV_SQRT_2));
    }
}

#[cfg(target_arch = "x86_64")]
fn is_avx2_fma_available() -> bool {
    use std::sync::atomic::{AtomicU8, Ordering};
    static CACHE: AtomicU8 = AtomicU8::new(0);
    match CACHE.load(Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => {
            let has = is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma");
            CACHE.store(if has { 1 } else { 2 }, Ordering::Relaxed);
            has
        }
    }
}

/// AVX2+FMA exp. Range-reduces x = n·ln2 + r, computes exp(r) by a
/// 5th-order polynomial, then multiplies by 2^n via bit manipulation
/// of the IEEE-754 exponent. Accurate to ~2 ULPs over [-87, 87].
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "fma")]
unsafe fn exp_avx2(x: std::arch::x86_64::__m256) -> std::arch::x86_64::__m256 {
    use std::arch::x86_64::*;
    // Clamp to avoid 2^n overflow/underflow.
    let x = unsafe { _mm256_min_ps(x, _mm256_set1_ps(88.0)) };
    let x = unsafe { _mm256_max_ps(x, _mm256_set1_ps(-88.0)) };

    let log2e = unsafe { _mm256_set1_ps(std::f32::consts::LOG2_E) };
    let ln2 = unsafe { _mm256_set1_ps(std::f32::consts::LN_2) };

    // n = round(x · log2(e))
    let n = unsafe {
        _mm256_round_ps::<{ _MM_FROUND_TO_NEAREST_INT | _MM_FROUND_NO_EXC }>(_mm256_mul_ps(
            x, log2e,
        ))
    };
    // r = x − n·ln(2)
    let r = unsafe { _mm256_fnmadd_ps(n, ln2, x) };

    // 5th-order minimax polynomial for exp(r), r ∈ [−ln2/2, ln2/2].
    let c5 = unsafe { _mm256_set1_ps(0.008_333_334_f32) };
    let c4 = unsafe { _mm256_set1_ps(0.041_666_664_f32) };
    let c3 = unsafe { _mm256_set1_ps(0.166_666_67_f32) };
    let c2 = unsafe { _mm256_set1_ps(0.5_f32) };
    let c1 = unsafe { _mm256_set1_ps(1.0_f32) };
    let c0 = unsafe { _mm256_set1_ps(1.0_f32) };
    let mut p = unsafe { _mm256_fmadd_ps(c5, r, c4) };
    p = unsafe { _mm256_fmadd_ps(p, r, c3) };
    p = unsafe { _mm256_fmadd_ps(p, r, c2) };
    p = unsafe { _mm256_fmadd_ps(p, r, c1) };
    p = unsafe { _mm256_fmadd_ps(p, r, c0) };

    // 2^n via `(n + 127) << 23` bit pattern.
    let n_i = unsafe { _mm256_cvtps_epi32(n) };
    let exp_bits =
        unsafe { _mm256_slli_epi32::<23>(_mm256_add_epi32(n_i, _mm256_set1_epi32(127))) };
    let two_n = unsafe { _mm256_castsi256_ps(exp_bits) };
    unsafe { _mm256_mul_ps(p, two_n) }
}

/// AVX2 vectorized exact GELU. Computes
/// `0.5 · x · (1 + erf(x · 1/√2))` using the same A&S 7.1.26 erf
/// approximation as the scalar path, with the exp(-z²) call replaced
/// by `exp_avx2`. Output matches scalar within ~1e-6.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "fma")]
unsafe fn gelu_inplace_avx2(x: &mut [f32]) {
    use std::arch::x86_64::*;
    let len = x.len();
    let chunks = len / 8;
    let tail_start = chunks * 8;

    let inv_sqrt2 = unsafe { _mm256_set1_ps(std::f32::consts::FRAC_1_SQRT_2) };
    let half = unsafe { _mm256_set1_ps(0.5) };
    let one = unsafe { _mm256_set1_ps(1.0) };
    let sign_mask_v = unsafe { _mm256_set1_ps(-0.0) }; // 0x80000000 bit pattern
    let p_v = unsafe { _mm256_set1_ps(ERF_P) };
    let a1 = unsafe { _mm256_set1_ps(ERF_A1) };
    let a2 = unsafe { _mm256_set1_ps(ERF_A2) };
    let a3 = unsafe { _mm256_set1_ps(ERF_A3) };
    let a4 = unsafe { _mm256_set1_ps(ERF_A4) };
    let a5 = unsafe { _mm256_set1_ps(ERF_A5) };
    let neg_one = unsafe { _mm256_set1_ps(-1.0) };

    for c in 0..chunks {
        let xv = unsafe { _mm256_loadu_ps(x.as_ptr().add(c * 8)) };
        let z = unsafe { _mm256_mul_ps(xv, inv_sqrt2) };
        // sign bit of z; |z|
        let sign = unsafe { _mm256_and_ps(z, sign_mask_v) };
        let az = unsafe { _mm256_andnot_ps(sign_mask_v, z) };
        // t = 1 / (1 + p·|z|)
        let t = unsafe { _mm256_div_ps(one, _mm256_fmadd_ps(p_v, az, one)) };
        // poly = ((((a5·t + a4)·t + a3)·t + a2)·t + a1)·t
        let mut poly = unsafe { _mm256_fmadd_ps(a5, t, a4) };
        poly = unsafe { _mm256_fmadd_ps(poly, t, a3) };
        poly = unsafe { _mm256_fmadd_ps(poly, t, a2) };
        poly = unsafe { _mm256_fmadd_ps(poly, t, a1) };
        poly = unsafe { _mm256_mul_ps(poly, t) };
        // exp(-z²) = exp(-|z|²)
        let neg_z2 = unsafe { _mm256_mul_ps(_mm256_mul_ps(az, az), neg_one) };
        let exp_term = unsafe { exp_avx2(neg_z2) };
        // y = 1 − poly·exp_term
        let y = unsafe { _mm256_sub_ps(one, _mm256_mul_ps(poly, exp_term)) };
        // Apply sign of z by xor-ing with sign bit.
        let erf_z = unsafe { _mm256_xor_ps(y, sign) };
        // gelu = 0.5·x·(1 + erf_z)
        let out = unsafe { _mm256_mul_ps(_mm256_mul_ps(half, xv), _mm256_add_ps(one, erf_z)) };
        unsafe { _mm256_storeu_ps(x.as_mut_ptr().add(c * 8), out) };
    }

    // Scalar tail.
    for v in &mut x[tail_start..] {
        *v = 0.5 * *v * (1.0 + erf(*v * std::f32::consts::FRAC_1_SQRT_2));
    }
}

// Abramowitz & Stegun 7.1.26 erf approximation coefficients.
// erf(x) ≈ sign(x) · (1 − ((((A5·t + A4)·t + A3)·t + A2)·t + A1)·t·exp(−x²))
// where t = 1/(1 + P·|x|). Max abs error ~1.5e-7, well within f32.
// Kept in published 8-digit form for greppability against the
// literature, even though f32 rounds the trailing digit away.
// Used by both scalar [`erf`] and AVX2 [`gelu_inplace_avx2`].
#[allow(clippy::excessive_precision)]
const ERF_A1: f32 = 0.254_829_59;
#[allow(clippy::excessive_precision)]
const ERF_A2: f32 = -0.284_496_73;
#[allow(clippy::excessive_precision)]
const ERF_A3: f32 = 1.421_413_74;
#[allow(clippy::excessive_precision)]
const ERF_A4: f32 = -1.453_152_03;
#[allow(clippy::excessive_precision)]
const ERF_A5: f32 = 1.061_405_43;
const ERF_P: f32 = 0.327_591_1;

/// Scalar A&S 7.1.26 erf approximation. See the `ERF_*` constants above.
fn erf(x: f32) -> f32 {
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let ax = x.abs();
    let t = 1.0 / (1.0 + ERF_P * ax);
    let y = 1.0
        - (((((ERF_A5 * t + ERF_A4) * t) + ERF_A3) * t + ERF_A2) * t + ERF_A1)
            * t
            * (-ax * ax).exp();
    sign * y
}

/// In-place softmax along the last axis of a `[rows, cols]` tensor.
/// Numerically stable via the standard max-subtract trick.
pub fn softmax_inplace(x: &mut [f32], rows: usize, cols: usize) {
    debug_assert_eq!(x.len(), rows * cols);
    for r in 0..rows {
        let row = &mut x[r * cols..(r + 1) * cols];
        let max = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let mut sum = 0.0f32;
        for v in row.iter_mut() {
            *v = (*v - max).exp();
            sum += *v;
        }
        let inv_sum = 1.0 / sum;
        for v in row.iter_mut() {
            *v *= inv_sum;
        }
    }
}

/// Attention-masked mean pooling. Input `x` is `[rows, cols]`; mask
/// is `[rows]` (1.0 for real tokens, 0.0 for padding). Output is
/// `[cols]` (the masked-mean over rows).
pub fn masked_mean_pool(x: &[f32], rows: usize, cols: usize, mask: &[f32]) -> Vec<f32> {
    debug_assert_eq!(x.len(), rows * cols);
    debug_assert_eq!(mask.len(), rows);
    let mut out = vec![0.0f32; cols];
    let mut total = 0.0f32;
    for r in 0..rows {
        let m = mask[r];
        if m == 0.0 {
            continue;
        }
        total += m;
        let row = &x[r * cols..(r + 1) * cols];
        for (o, &v) in out.iter_mut().zip(row.iter()) {
            *o += v * m;
        }
    }
    let denom = total.max(1e-9);
    for o in out.iter_mut() {
        *o /= denom;
    }
    out
}

/// L2-normalize a vector in place. Returns nothing.
pub fn l2_normalize_inplace(v: &mut [f32]) {
    let mut sum_sq = 0.0f32;
    for x in v.iter() {
        sum_sq += x * x;
    }
    let norm = sum_sq.sqrt() + 1e-12;
    for x in v.iter_mut() {
        *x /= norm;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matmul_2x3_3x2() {
        // A = [[1, 2, 3], [4, 5, 6]] (2x3)
        // B = [[7, 8], [9, 10], [11, 12]] (3x2)
        // A·B = [[58, 64], [139, 154]]
        let a = vec![1., 2., 3., 4., 5., 6.];
        let b = vec![7., 8., 9., 10., 11., 12.];
        let mut c = vec![0.0; 4];
        matmul(2, 3, 2, &a, &b, &mut c);
        assert_eq!(c, vec![58., 64., 139., 154.]);
    }

    #[test]
    fn matmul_at_bt_matches_explicit_transpose() {
        // A = [[1, 2, 3], [4, 5, 6]] (2x3)
        // B (as [n, k]) = [[7, 9, 11], [8, 10, 12]] (2x3)
        //   → B^T (as [k, n]) = [[7, 8], [9, 10], [11, 12]]
        //   → A·B^T = same as A·(B^T as [3x2]) = [[58, 64], [139, 154]]
        let a = vec![1., 2., 3., 4., 5., 6.];
        let b_t = vec![7., 9., 11., 8., 10., 12.]; // [n=2, k=3]
        let mut c = vec![0.0; 4];
        matmul_at_bt(2, 3, 2, &a, &b_t, &mut c);
        assert_eq!(c, vec![58., 64., 139., 154.]);
    }

    #[test]
    fn layernorm_zero_input_yields_beta() {
        // Zero input has mean 0, var 0 → eps in denominator → result
        // is `(0 - 0) * 1/sqrt(eps) * gamma + beta = beta`.
        let mut x = vec![0.0; 4];
        let gamma = vec![1.0, 2.0, 3.0, 4.0];
        let beta = vec![10., 20., 30., 40.];
        layernorm_inplace(&mut x, 1, 4, &gamma, &beta, 1e-12);
        assert_eq!(x, beta);
    }

    #[test]
    fn layernorm_normalizes_per_row() {
        // Row [1, 2, 3, 4] → mean 2.5, var 1.25.
        // Normalized = [-1.342, -0.447, 0.447, 1.342].
        let mut x = vec![1., 2., 3., 4.];
        let gamma = vec![1.0; 4];
        let beta = vec![0.0; 4];
        layernorm_inplace(&mut x, 1, 4, &gamma, &beta, 1e-12);
        let expected = [-1.341_640_8, -0.447_213_6, 0.447_213_6, 1.341_640_8];
        for (a, b) in x.iter().zip(expected.iter()) {
            assert!((a - b).abs() < 1e-5, "got {a}, expected {b}");
        }
        // Row sum should be ~0.
        let sum: f32 = x.iter().sum();
        assert!(sum.abs() < 1e-5);
    }

    #[test]
    fn gelu_known_values() {
        let mut x = vec![0.0, 1.0, -1.0, 2.0];
        gelu_inplace(&mut x);
        // Reference: gelu(0)=0, gelu(1)≈0.8413, gelu(-1)≈-0.1587, gelu(2)≈1.9545
        let expected = [0.0, 0.8413, -0.1587, 1.9545];
        for (a, b) in x.iter().zip(expected.iter()) {
            assert!((a - b).abs() < 1e-3, "got {a}, expected {b}");
        }
    }

    #[test]
    fn layernorm_avx2_matches_scalar() {
        let lcg = |x: u32| x.wrapping_mul(1664525).wrapping_add(1013904223);
        let mut state = 99u32;
        let mut next_f32 = || {
            state = lcg(state);
            (state as f32 / u32::MAX as f32) * 4.0 - 2.0
        };
        for &(rows, cols) in &[(1, 8), (3, 16), (5, 17), (4, 384), (13, 384)] {
            let mut x_scalar: Vec<f32> = (0..rows * cols).map(|_| next_f32()).collect();
            let mut x_simd = x_scalar.clone();
            let gamma: Vec<f32> = (0..cols).map(|i| 0.9 + (i as f32) * 0.01).collect();
            let beta: Vec<f32> = (0..cols).map(|i| 0.1 + (i as f32) * 0.005).collect();
            layernorm_inplace(&mut x_simd, rows, cols, &gamma, &beta, 1e-12);
            layernorm_inplace_scalar(&mut x_scalar, rows, cols, &gamma, &beta, 1e-12);
            for (i, (a, b)) in x_simd.iter().zip(x_scalar.iter()).enumerate() {
                assert!(
                    (a - b).abs() < 5e-5,
                    "layernorm mismatch rows={rows} cols={cols} i={i}: simd={a} scalar={b}"
                );
            }
        }
    }

    #[test]
    fn gelu_avx2_matches_scalar() {
        // Mixed magnitudes spanning the GELU response curve, plus a
        // misaligned tail to exercise the scalar fall-through.
        let inputs: Vec<f32> = (0..37).map(|i| (i as f32 - 18.0) * 0.21).collect();
        let mut x_simd = inputs.clone();
        let mut x_scalar = inputs.clone();
        gelu_inplace(&mut x_simd);
        gelu_inplace_scalar(&mut x_scalar);
        for (i, (s, r)) in x_simd.iter().zip(x_scalar.iter()).enumerate() {
            assert!(
                (s - r).abs() < 5e-6,
                "gelu mismatch at i={i}: simd={s} scalar={r}"
            );
        }
    }

    #[test]
    fn softmax_distribution_sums_to_one() {
        let mut x = vec![1.0, 2.0, 3.0, 4.0];
        softmax_inplace(&mut x, 1, 4);
        let sum: f32 = x.iter().sum();
        assert!((sum - 1.0).abs() < 1e-6, "sum = {sum}");
        // Highest input gets the largest probability.
        assert!(x[3] > x[2] && x[2] > x[1] && x[1] > x[0]);
    }

    #[test]
    fn softmax_stable_for_large_inputs() {
        // Without max-subtract, exp(1000) overflows. Verify stability.
        let mut x = vec![1000.0, 1001.0, 1002.0];
        softmax_inplace(&mut x, 1, 3);
        let sum: f32 = x.iter().sum();
        assert!((sum - 1.0).abs() < 1e-6);
        assert!(x.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn masked_mean_pool_only_real_tokens() {
        // x = [[1, 2], [3, 4], [5, 6]], mask = [1, 1, 0]
        // → masked mean = [(1+3)/2, (2+4)/2] = [2, 3]
        let x = vec![1., 2., 3., 4., 5., 6.];
        let mask = vec![1.0, 1.0, 0.0];
        let out = masked_mean_pool(&x, 3, 2, &mask);
        assert_eq!(out, vec![2., 3.]);
    }

    #[test]
    fn l2_normalize_yields_unit_vector() {
        let mut v = vec![3.0, 4.0];
        l2_normalize_inplace(&mut v);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-6);
        assert!((v[0] - 0.6).abs() < 1e-6);
        assert!((v[1] - 0.8).abs() < 1e-6);
    }
}
