//! INT8 quantized matmul kernels and the surrounding
//! quantize/dequantize plumbing.

// Rust 2024 makes `unsafe_op_in_unsafe_fn` warn-by-default, which
// effectively demands explicit unsafe blocks even inside unsafe fns.
// Combined with `unused_unsafe` it gives contradictory advice on the
// AVX2/VNNI intrinsics — we silence the noisier of the two here.
#![allow(unused_unsafe)]
//!
//! ── Quantization scheme (matches `examples/quantize_to_int8.rs`) ──
//!
//! - **Weights**: symmetric, per-output-channel. Stored **column-major**
//!   as `[i8; in_dim * out_dim]` where column j is at offsets
//!   `j*in_dim .. (j+1)*in_dim` contiguously. One f32 scale per column.
//! - **Activations**: symmetric, **per-row** (per-token), computed
//!   dynamically per matmul. Each row of the activation matrix gets
//!   its own scale, preserving per-token dynamic range.
//!
//! Dequantization formula (same for all kernels):
//!
//! ```text
//! out_fp32[i, j] = a_scale[i] · w_scale[j] · Σ_k a_i8[i, k] · w_i8[k, j]
//!                  + bias[j]
//! ```
//!
//! ── Matmul kernels ──────────────────────────────────────────────────
//!
//! All three kernels return the **same** `Σ a_i8 * w_i8` in `c[i, j]`
//! so the dequant formula is uniform:
//!
//! 1. `matmul_i8_scalar` — naive triple-loop reference.
//! 2. `matmul_i8_avx2` — AVX2 widen-to-i16 + `pmaddwd`, for CPUs that
//!    have AVX2 but not VNNI.
//! 3. `matmul_i8_vnni` — AVX-VNNI `vpdpbusd` fast path. Takes
//!    activations in u8 form (shifted by +128) and subtracts the
//!    shift correction internally so the output matches the other
//!    kernels.
//!
//! Dispatch picks the fastest available; runtime CPUID-detected.

use crate::inference::weights_int8::QuantWeight;

/// Reusable scratch buffers for `quantized_matmul`. Carry them through
/// a forward pass to avoid per-call allocations.
#[derive(Default)]
pub struct QMatmulScratch {
    pub act_i8: Vec<i8>,
    pub act_u8: Vec<u8>,
    pub acc: Vec<i32>,
    pub a_scales: Vec<f32>,
    // Tracks which fp32 input the scratch was last quantized for so
    // callers running multiple matmuls against the same input
    // (Q/K/V projection over the same `x`) can skip re-quantizing.
    // Hash is a 64-bit FNV-style digest over the input pointer + len
    // + first/last fp32 value; collisions are astronomically rare in
    // the per-layer hot path.
    pub last_input_token: u64,
}

/// Quantize an activation matrix into `scratch` once, then call
/// `quantized_matmul_prequant` repeatedly with the same scratch and
/// different weights/biases. Saves repeated quantization work when
/// the same activation feeds multiple parallel projections (Q, K, V).
pub fn quantize_activation(x: &[f32], m: usize, k: usize, scratch: &mut QMatmulScratch) {
    debug_assert_eq!(x.len(), m * k);
    scratch.a_scales.clear();
    scratch.a_scales.resize(m, 0.0);
    scratch.act_i8.clear();
    scratch.act_i8.resize(m * k, 0);
    scratch.act_u8.clear();
    scratch.act_u8.resize(m * k, 0);

    #[cfg(target_arch = "x86_64")]
    {
        if is_avx2_available() {
            for i in 0..m {
                let row = &x[i * k..(i + 1) * k];
                let i8_dst = &mut scratch.act_i8[i * k..(i + 1) * k];
                let u8_dst = &mut scratch.act_u8[i * k..(i + 1) * k];
                let a_scale = unsafe { quantize_row_avx2(row, i8_dst, u8_dst) };
                scratch.a_scales[i] = a_scale;
            }
            scratch.last_input_token = input_token(x, m, k);
            return;
        }
    }

    for i in 0..m {
        let row = &x[i * k..(i + 1) * k];
        let row_max_abs = row.iter().copied().map(f32::abs).fold(0.0f32, f32::max);
        let a_scale = (row_max_abs / 127.0).max(1e-12);
        scratch.a_scales[i] = a_scale;
        let inv_scale = 1.0 / a_scale;
        #[allow(clippy::needless_range_loop)]
        for kk in 0..k {
            let q = (row[kk] * inv_scale).round().clamp(-127.0, 127.0) as i32;
            scratch.act_i8[i * k + kk] = q as i8;
            scratch.act_u8[i * k + kk] = (q + 128) as u8;
        }
    }
    // Hash the input identity so callers can detect re-use without
    // explicit cache invalidation.
    scratch.last_input_token = input_token(x, m, k);
}

/// AVX2 per-row quantize. Two passes: abs-max reduction, then
/// scale-multiply + round + cast. Output is bit-equivalent to the
/// scalar path up to AVX2's round-half-to-even (vs Rust's round-half-
/// away-from-zero) — a difference that flips at most 1 LSB per element
/// and is invisible after the per-tensor scale.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn quantize_row_avx2(row: &[f32], a_i8: &mut [i8], a_u8: &mut [u8]) -> f32 {
    use std::arch::x86_64::*;
    let k = row.len();
    debug_assert_eq!(a_i8.len(), k);
    debug_assert_eq!(a_u8.len(), k);
    let chunks = k / 8;
    let tail_start = chunks * 8;

    // ── Pass 1: row max-abs ────────────────────────────────────────
    let sign_mask = unsafe { _mm256_castsi256_ps(_mm256_set1_epi32(0x7FFFFFFFu32 as i32)) };
    let mut max_v = unsafe { _mm256_setzero_ps() };
    for c in 0..chunks {
        let v = unsafe { _mm256_loadu_ps(row.as_ptr().add(c * 8)) };
        let abs_v = unsafe { _mm256_and_ps(v, sign_mask) };
        max_v = unsafe { _mm256_max_ps(max_v, abs_v) };
    }
    // Horizontal max across the 8 lanes.
    let mut tmp = [0.0f32; 8];
    unsafe { _mm256_storeu_ps(tmp.as_mut_ptr(), max_v) };
    let mut max_abs = tmp.iter().copied().fold(0.0f32, f32::max);
    #[allow(clippy::needless_range_loop)]
    for kk in tail_start..k {
        max_abs = max_abs.max(row[kk].abs());
    }

    let a_scale = (max_abs / 127.0).max(1e-12);
    let inv_scale = 1.0 / a_scale;

    // ── Pass 2: scale, round, clamp, narrow to i8/u8 ───────────────
    let inv_scale_v = unsafe { _mm256_set1_ps(inv_scale) };
    let clamp_hi = unsafe { _mm256_set1_ps(127.0) };
    let clamp_lo = unsafe { _mm256_set1_ps(-127.0) };
    let shift = unsafe { _mm256_set1_epi32(128) };

    for c in 0..chunks {
        let v = unsafe { _mm256_loadu_ps(row.as_ptr().add(c * 8)) };
        let scaled = unsafe { _mm256_mul_ps(v, inv_scale_v) };
        let clamped = unsafe { _mm256_max_ps(_mm256_min_ps(scaled, clamp_hi), clamp_lo) };
        let q32 = unsafe { _mm256_cvtps_epi32(clamped) };
        let u32_v = unsafe { _mm256_add_epi32(q32, shift) };

        // Narrow 8×i32 → 8×i8. `packs_epi32` operates per 128-bit lane,
        // so `lo32(packed_lo)` holds q0..3 and `lo32(packed_hi)` holds
        // q4..7 — concatenate the two u32 halves into a single u64
        // store.
        let i16x = unsafe { _mm256_packs_epi32(q32, q32) };
        let i8x = unsafe { _mm256_packs_epi16(i16x, i16x) };
        let lo = unsafe { _mm256_castsi256_si128(i8x) };
        let hi = unsafe { _mm256_extracti128_si256::<1>(i8x) };
        let lo32 = unsafe { _mm_cvtsi128_si32(lo) } as u32;
        let hi32 = unsafe { _mm_cvtsi128_si32(hi) } as u32;
        let bytes_i = lo32 as u64 | ((hi32 as u64) << 32);
        unsafe {
            (a_i8.as_mut_ptr().add(c * 8) as *mut u64).write_unaligned(bytes_i);
        }

        // u8 path: values are in [1, 255] which fits i16, so we can
        // packs_epi32 → packus_epi16 (saturating unsigned to u8).
        let i16u = unsafe { _mm256_packs_epi32(u32_v, u32_v) };
        let u8x = unsafe { _mm256_packus_epi16(i16u, i16u) };
        let lo = unsafe { _mm256_castsi256_si128(u8x) };
        let hi = unsafe { _mm256_extracti128_si256::<1>(u8x) };
        let lo32 = unsafe { _mm_cvtsi128_si32(lo) } as u32;
        let hi32 = unsafe { _mm_cvtsi128_si32(hi) } as u32;
        let bytes_u = lo32 as u64 | ((hi32 as u64) << 32);
        unsafe {
            (a_u8.as_mut_ptr().add(c * 8) as *mut u64).write_unaligned(bytes_u);
        }
    }

    // Scalar tail.
    for kk in tail_start..k {
        let q = (row[kk] * inv_scale).round().clamp(-127.0, 127.0) as i32;
        a_i8[kk] = q as i8;
        a_u8[kk] = (q + 128) as u8;
    }

    a_scale
}

/// Matmul against an already-quantized activation in `scratch`. Use
/// `quantize_activation` first to populate the scratch.
pub fn quantized_matmul_prequant(
    m: usize,
    k: usize,
    n: usize,
    w: &QuantWeight,
    bias: &[f32],
    out: &mut [f32],
    scratch: &mut QMatmulScratch,
) {
    debug_assert_eq!(w.in_dim, k);
    debug_assert_eq!(w.out_dim, n);
    debug_assert_eq!(bias.len(), n);
    debug_assert_eq!(out.len(), m * n);
    debug_assert_eq!(scratch.a_scales.len(), m);

    scratch.acc.clear();
    scratch.acc.resize(m * n, 0);
    matmul_i8_dispatch(
        m,
        k,
        n,
        &scratch.act_i8,
        &scratch.act_u8,
        &w.q_data,
        &w.col_sums,
        &mut scratch.acc,
    );
    for i in 0..m {
        let a_scale = scratch.a_scales[i];
        let row_acc = &scratch.acc[i * n..(i + 1) * n];
        let row_out = &mut out[i * n..(i + 1) * n];
        for j in 0..n {
            row_out[j] = (row_acc[j] as f32) * (a_scale * w.scales[j]) + bias[j];
        }
    }
}

/// Hash that lets us detect activation re-use without an explicit cache
/// invalidation flag. Mixes the input slice's first/last/length so
/// collisions are astronomically unlikely for typical forward-pass use.
fn input_token(x: &[f32], m: usize, k: usize) -> u64 {
    let len = m * k;
    if len == 0 {
        return 0;
    }
    let first = x[0].to_bits() as u64;
    let last = x[len - 1].to_bits() as u64;
    let mid = x[len / 2].to_bits() as u64;
    first.wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left(13)
        ^ last.wrapping_mul(0xBF58_476D_1CE4_E5B9).rotate_left(31)
        ^ mid.wrapping_mul(0x94D0_49BB_1331_11EB).rotate_left(7)
        ^ (len as u64)
}

/// Quantize fp32 activation `x` to per-row INT8, run i8 matmul vs the
/// column-major INT8 weight `w`, then dequantize + add bias. Output
/// `out` is `[m, n]` fp32 row-major.
///
/// Internally consults `scratch.last_input_token` to skip re-
/// quantization when called with the same `x` repeatedly — this
/// matters for Q/K/V projections, which all run against the same
/// post-LayerNorm activation.
#[allow(clippy::too_many_arguments)]
pub fn quantized_matmul(
    x: &[f32],
    m: usize,
    k: usize,
    n: usize,
    w: &QuantWeight,
    bias: &[f32],
    out: &mut [f32],
    scratch: &mut QMatmulScratch,
) {
    debug_assert_eq!(x.len(), m * k);
    let tok = input_token(x, m, k);
    if scratch.last_input_token != tok || scratch.a_scales.len() != m {
        quantize_activation(x, m, k, scratch);
        // `quantize_activation` already stamps last_input_token.
    }
    quantized_matmul_prequant(m, k, n, w, bias, out, scratch);
}

/// Dispatch to the fastest available INT8 matmul kernel. All kernels
/// produce `c[i, j] = Σ_k a_i8[i, k] · b[k, j]` regardless of which
/// path runs.
#[allow(clippy::too_many_arguments)]
fn matmul_i8_dispatch(
    m: usize,
    k: usize,
    n: usize,
    a_i8: &[i8],
    a_u8: &[u8],
    b: &[i8],
    col_sums: &[i32],
    c: &mut [i32],
) {
    #[cfg(target_arch = "x86_64")]
    {
        if is_avxvnni_available() {
            unsafe { matmul_i8_vnni(m, k, n, a_u8, b, col_sums, c) };
            return;
        }
        if is_avx2_available() {
            unsafe { matmul_i8_avx2(m, k, n, a_i8, b, c) };
            return;
        }
    }
    let _ = a_u8;
    let _ = col_sums;
    matmul_i8_scalar(m, k, n, a_i8, b, c);
}

/// Naive scalar implementation. Slow but obviously correct — the
/// reference all other paths must match.
pub fn matmul_i8_scalar(m: usize, k: usize, n: usize, a: &[i8], b: &[i8], c: &mut [i32]) {
    debug_assert_eq!(a.len(), m * k);
    debug_assert_eq!(b.len(), k * n);
    debug_assert_eq!(c.len(), m * n);
    for i in 0..m {
        let a_row = &a[i * k..(i + 1) * k];
        for j in 0..n {
            let b_col = &b[j * k..(j + 1) * k];
            let mut acc: i32 = 0;
            for kk in 0..k {
                acc += (a_row[kk] as i32) * (b_col[kk] as i32);
            }
            c[i * n + j] = acc;
        }
    }
}

// ── x86_64 fast paths ────────────────────────────────────────────────

#[cfg(target_arch = "x86_64")]
fn is_avx2_available() -> bool {
    use std::sync::atomic::{AtomicU8, Ordering};
    static CACHE: AtomicU8 = AtomicU8::new(0);
    match CACHE.load(Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => {
            let has = is_x86_feature_detected!("avx2");
            CACHE.store(if has { 1 } else { 2 }, Ordering::Relaxed);
            has
        }
    }
}

#[cfg(target_arch = "x86_64")]
fn is_avxvnni_available() -> bool {
    use std::sync::atomic::{AtomicU8, Ordering};
    static CACHE: AtomicU8 = AtomicU8::new(0);
    match CACHE.load(Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => {
            let has = is_x86_feature_detected!("avxvnni");
            CACHE.store(if has { 1 } else { 2 }, Ordering::Relaxed);
            has
        }
    }
}

/// AVX2 (no VNNI) fast path: widen i8 → i16 then `pmaddwd`.
/// ~3× faster than scalar; ~3× slower than VNNI.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn matmul_i8_avx2(m: usize, k: usize, n: usize, a: &[i8], b: &[i8], c: &mut [i32]) {
    use std::arch::x86_64::*;
    debug_assert_eq!(a.len(), m * k);
    debug_assert_eq!(b.len(), k * n);
    debug_assert_eq!(c.len(), m * n);

    let chunks = k / 32;
    let tail_start = chunks * 32;
    let tail = k - tail_start;

    for i in 0..m {
        let a_row = unsafe { a.as_ptr().add(i * k) };
        for j in 0..n {
            let b_col = unsafe { b.as_ptr().add(j * k) };
            let mut acc_vec = unsafe { _mm256_setzero_si256() };
            for c_idx in 0..chunks {
                let off = c_idx * 32;
                let av = unsafe { _mm256_loadu_si256(a_row.add(off) as *const __m256i) };
                let bv = unsafe { _mm256_loadu_si256(b_col.add(off) as *const __m256i) };
                let a_lo = unsafe { _mm256_cvtepi8_epi16(_mm256_castsi256_si128(av)) };
                let a_hi = unsafe { _mm256_cvtepi8_epi16(_mm256_extracti128_si256(av, 1)) };
                let b_lo = unsafe { _mm256_cvtepi8_epi16(_mm256_castsi256_si128(bv)) };
                let b_hi = unsafe { _mm256_cvtepi8_epi16(_mm256_extracti128_si256(bv, 1)) };
                let prod_lo = unsafe { _mm256_madd_epi16(a_lo, b_lo) };
                let prod_hi = unsafe { _mm256_madd_epi16(a_hi, b_hi) };
                acc_vec = unsafe { _mm256_add_epi32(acc_vec, _mm256_add_epi32(prod_lo, prod_hi)) };
            }
            let mut acc = unsafe { hsum_i32_avx2(acc_vec) };
            for kk in 0..tail {
                let av = unsafe { *a_row.add(tail_start + kk) } as i32;
                let bv = unsafe { *b_col.add(tail_start + kk) } as i32;
                acc += av * bv;
            }
            c[i * n + j] = acc;
        }
    }
}

/// AVX-VNNI fast path. Uses `vpdpbusd` for a single-instruction
/// 32-element INT8 dot product. Tiled 2 rows × 4 columns: each (i,
/// k_chunk) loads 2 activation rows and 4 weight columns and updates
/// 8 i32 accumulators. Compared to a 1×4 tile, this reuses each B
/// column twice (across rows) — saves ~50% of B-column loads in the
/// hot path.
///
/// AVX2 has 16 ymm registers — we use 8 for accumulators, 4 for B,
/// 2 for A, 2 free. Fits.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avxvnni", enable = "avx2")]
unsafe fn matmul_i8_vnni(
    m: usize,
    k: usize,
    n: usize,
    a_u8: &[u8],
    b: &[i8],
    col_sums: &[i32],
    c: &mut [i32],
) {
    use std::arch::x86_64::*;
    debug_assert_eq!(a_u8.len(), m * k);
    debug_assert_eq!(b.len(), k * n);
    debug_assert_eq!(c.len(), m * n);
    debug_assert_eq!(col_sums.len(), n);

    let chunks = k / 32;
    let tail_start = chunks * 32;
    let tail = k - tail_start;

    let j_tile = 4usize;
    let j_main_end = (n / j_tile) * j_tile;
    let m_main_end = (m / 2) * 2;

    // 2×4 tile: handle pairs of rows together so each B column load is
    // reused twice. 8 ymm accumulators + 4 B + 2 A = 14 ymm, fits in 16.
    let mut i = 0;
    while i < m_main_end {
        let a_row0 = unsafe { a_u8.as_ptr().add(i * k) };
        let a_row1 = unsafe { a_u8.as_ptr().add((i + 1) * k) };
        let mut j = 0;
        while j < j_main_end {
            let mut acc00 = unsafe { _mm256_setzero_si256() };
            let mut acc01 = unsafe { _mm256_setzero_si256() };
            let mut acc02 = unsafe { _mm256_setzero_si256() };
            let mut acc03 = unsafe { _mm256_setzero_si256() };
            let mut acc10 = unsafe { _mm256_setzero_si256() };
            let mut acc11 = unsafe { _mm256_setzero_si256() };
            let mut acc12 = unsafe { _mm256_setzero_si256() };
            let mut acc13 = unsafe { _mm256_setzero_si256() };
            let b0 = unsafe { b.as_ptr().add(j * k) };
            let b1 = unsafe { b.as_ptr().add((j + 1) * k) };
            let b2 = unsafe { b.as_ptr().add((j + 2) * k) };
            let b3 = unsafe { b.as_ptr().add((j + 3) * k) };
            for c_idx in 0..chunks {
                let off = c_idx * 32;
                let av0 = unsafe { _mm256_loadu_si256(a_row0.add(off) as *const __m256i) };
                let av1 = unsafe { _mm256_loadu_si256(a_row1.add(off) as *const __m256i) };
                let bv0 = unsafe { _mm256_loadu_si256(b0.add(off) as *const __m256i) };
                let bv1 = unsafe { _mm256_loadu_si256(b1.add(off) as *const __m256i) };
                let bv2 = unsafe { _mm256_loadu_si256(b2.add(off) as *const __m256i) };
                let bv3 = unsafe { _mm256_loadu_si256(b3.add(off) as *const __m256i) };
                acc00 = unsafe { _mm256_dpbusd_avx_epi32(acc00, av0, bv0) };
                acc01 = unsafe { _mm256_dpbusd_avx_epi32(acc01, av0, bv1) };
                acc02 = unsafe { _mm256_dpbusd_avx_epi32(acc02, av0, bv2) };
                acc03 = unsafe { _mm256_dpbusd_avx_epi32(acc03, av0, bv3) };
                acc10 = unsafe { _mm256_dpbusd_avx_epi32(acc10, av1, bv0) };
                acc11 = unsafe { _mm256_dpbusd_avx_epi32(acc11, av1, bv1) };
                acc12 = unsafe { _mm256_dpbusd_avx_epi32(acc12, av1, bv2) };
                acc13 = unsafe { _mm256_dpbusd_avx_epi32(acc13, av1, bv3) };
            }
            let mut s00 = unsafe { hsum_i32_avx2(acc00) };
            let mut s01 = unsafe { hsum_i32_avx2(acc01) };
            let mut s02 = unsafe { hsum_i32_avx2(acc02) };
            let mut s03 = unsafe { hsum_i32_avx2(acc03) };
            let mut s10 = unsafe { hsum_i32_avx2(acc10) };
            let mut s11 = unsafe { hsum_i32_avx2(acc11) };
            let mut s12 = unsafe { hsum_i32_avx2(acc12) };
            let mut s13 = unsafe { hsum_i32_avx2(acc13) };
            for kk in 0..tail {
                let av0 = unsafe { *a_row0.add(tail_start + kk) } as i32;
                let av1 = unsafe { *a_row1.add(tail_start + kk) } as i32;
                let bv0 = unsafe { *b0.add(tail_start + kk) } as i32;
                let bv1 = unsafe { *b1.add(tail_start + kk) } as i32;
                let bv2 = unsafe { *b2.add(tail_start + kk) } as i32;
                let bv3 = unsafe { *b3.add(tail_start + kk) } as i32;
                s00 += av0 * bv0;
                s01 += av0 * bv1;
                s02 += av0 * bv2;
                s03 += av0 * bv3;
                s10 += av1 * bv0;
                s11 += av1 * bv1;
                s12 += av1 * bv2;
                s13 += av1 * bv3;
            }
            // Subtract the +128 shift correction: shifted = unshifted + 128 * col_sum.
            let cs0 = col_sums[j];
            let cs1 = col_sums[j + 1];
            let cs2 = col_sums[j + 2];
            let cs3 = col_sums[j + 3];
            c[i * n + j] = s00 - 128 * cs0;
            c[i * n + j + 1] = s01 - 128 * cs1;
            c[i * n + j + 2] = s02 - 128 * cs2;
            c[i * n + j + 3] = s03 - 128 * cs3;
            c[(i + 1) * n + j] = s10 - 128 * cs0;
            c[(i + 1) * n + j + 1] = s11 - 128 * cs1;
            c[(i + 1) * n + j + 2] = s12 - 128 * cs2;
            c[(i + 1) * n + j + 3] = s13 - 128 * cs3;
            j += j_tile;
        }
        // j-tail: remaining columns, both rows.
        while j < n {
            let mut acc0 = unsafe { _mm256_setzero_si256() };
            let mut acc1 = unsafe { _mm256_setzero_si256() };
            let b_col = unsafe { b.as_ptr().add(j * k) };
            for c_idx in 0..chunks {
                let off = c_idx * 32;
                let av0 = unsafe { _mm256_loadu_si256(a_row0.add(off) as *const __m256i) };
                let av1 = unsafe { _mm256_loadu_si256(a_row1.add(off) as *const __m256i) };
                let bv = unsafe { _mm256_loadu_si256(b_col.add(off) as *const __m256i) };
                acc0 = unsafe { _mm256_dpbusd_avx_epi32(acc0, av0, bv) };
                acc1 = unsafe { _mm256_dpbusd_avx_epi32(acc1, av1, bv) };
            }
            let mut s0 = unsafe { hsum_i32_avx2(acc0) };
            let mut s1 = unsafe { hsum_i32_avx2(acc1) };
            for kk in 0..tail {
                let av0 = unsafe { *a_row0.add(tail_start + kk) } as i32;
                let av1 = unsafe { *a_row1.add(tail_start + kk) } as i32;
                let bv = unsafe { *b_col.add(tail_start + kk) } as i32;
                s0 += av0 * bv;
                s1 += av1 * bv;
            }
            let cs = col_sums[j];
            c[i * n + j] = s0 - 128 * cs;
            c[(i + 1) * n + j] = s1 - 128 * cs;
            j += 1;
        }
        i += 2;
    }

    // Handle odd trailing row (if any) with a 1×4 tile.
    while i < m {
        let a_row = unsafe { a_u8.as_ptr().add(i * k) };
        let mut j = 0;
        while j < j_main_end {
            let mut acc0 = unsafe { _mm256_setzero_si256() };
            let mut acc1 = unsafe { _mm256_setzero_si256() };
            let mut acc2 = unsafe { _mm256_setzero_si256() };
            let mut acc3 = unsafe { _mm256_setzero_si256() };
            let b0 = unsafe { b.as_ptr().add(j * k) };
            let b1 = unsafe { b.as_ptr().add((j + 1) * k) };
            let b2 = unsafe { b.as_ptr().add((j + 2) * k) };
            let b3 = unsafe { b.as_ptr().add((j + 3) * k) };
            for c_idx in 0..chunks {
                let off = c_idx * 32;
                let av = unsafe { _mm256_loadu_si256(a_row.add(off) as *const __m256i) };
                let bv0 = unsafe { _mm256_loadu_si256(b0.add(off) as *const __m256i) };
                let bv1 = unsafe { _mm256_loadu_si256(b1.add(off) as *const __m256i) };
                let bv2 = unsafe { _mm256_loadu_si256(b2.add(off) as *const __m256i) };
                let bv3 = unsafe { _mm256_loadu_si256(b3.add(off) as *const __m256i) };
                acc0 = unsafe { _mm256_dpbusd_avx_epi32(acc0, av, bv0) };
                acc1 = unsafe { _mm256_dpbusd_avx_epi32(acc1, av, bv1) };
                acc2 = unsafe { _mm256_dpbusd_avx_epi32(acc2, av, bv2) };
                acc3 = unsafe { _mm256_dpbusd_avx_epi32(acc3, av, bv3) };
            }
            let mut sum0 = unsafe { hsum_i32_avx2(acc0) };
            let mut sum1 = unsafe { hsum_i32_avx2(acc1) };
            let mut sum2 = unsafe { hsum_i32_avx2(acc2) };
            let mut sum3 = unsafe { hsum_i32_avx2(acc3) };
            for kk in 0..tail {
                let av = unsafe { *a_row.add(tail_start + kk) } as i32;
                sum0 += av * (unsafe { *b0.add(tail_start + kk) } as i32);
                sum1 += av * (unsafe { *b1.add(tail_start + kk) } as i32);
                sum2 += av * (unsafe { *b2.add(tail_start + kk) } as i32);
                sum3 += av * (unsafe { *b3.add(tail_start + kk) } as i32);
            }
            c[i * n + j] = sum0 - 128 * col_sums[j];
            c[i * n + j + 1] = sum1 - 128 * col_sums[j + 1];
            c[i * n + j + 2] = sum2 - 128 * col_sums[j + 2];
            c[i * n + j + 3] = sum3 - 128 * col_sums[j + 3];
            j += j_tile;
        }
        while j < n {
            let mut acc = unsafe { _mm256_setzero_si256() };
            let b_col = unsafe { b.as_ptr().add(j * k) };
            for c_idx in 0..chunks {
                let off = c_idx * 32;
                let av = unsafe { _mm256_loadu_si256(a_row.add(off) as *const __m256i) };
                let bv = unsafe { _mm256_loadu_si256(b_col.add(off) as *const __m256i) };
                acc = unsafe { _mm256_dpbusd_avx_epi32(acc, av, bv) };
            }
            let mut sum = unsafe { hsum_i32_avx2(acc) };
            for kk in 0..tail {
                let av = unsafe { *a_row.add(tail_start + kk) } as i32;
                sum += av * (unsafe { *b_col.add(tail_start + kk) } as i32);
            }
            c[i * n + j] = sum - 128 * col_sums[j];
            j += 1;
        }
        i += 1;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn hsum_i32_avx2(v: std::arch::x86_64::__m256i) -> i32 {
    use std::arch::x86_64::*;
    let lo128 = unsafe { _mm256_castsi256_si128(v) };
    let hi128 = unsafe { _mm256_extracti128_si256(v, 1) };
    let sum128 = unsafe { _mm_add_epi32(lo128, hi128) };
    let shuf = unsafe { _mm_shuffle_epi32(sum128, 0b_01_00_11_10) };
    let sum64 = unsafe { _mm_add_epi32(sum128, shuf) };
    let shuf2 = unsafe { _mm_shuffle_epi32(sum64, 0b_10_11_00_01) };
    let sum32 = unsafe { _mm_add_epi32(sum64, shuf2) };
    unsafe { _mm_cvtsi128_si32(sum32) }
}

// ── Embedding dequantization ────────────────────────────────────────

/// Dequantize one row (one embedding lookup) of a per-tensor quantized
/// embedding table.
pub fn dequant_embedding_row(q_data: &[i8], scale: f32, cols: usize, row: usize, out: &mut [f32]) {
    debug_assert_eq!(out.len(), cols);
    let src = &q_data[row * cols..(row + 1) * cols];
    for (o, &q) in out.iter_mut().zip(src.iter()) {
        *o = q as f32 * scale;
    }
}

/// Fused word + position + type-0 embedding dequant + sum, in a single
/// SIMD pass per token. Replaces three `dequant_embedding_row` calls
/// plus a fp32 add loop with a single fused loop — eliminates two
/// passes over fp32 memory.
#[allow(clippy::too_many_arguments)]
pub fn dequant_and_sum_token_embedding(
    word_q: &[i8],
    word_scale: f32,
    word_row: usize,
    pos_q: &[i8],
    pos_scale: f32,
    pos_row: usize,
    type_q: &[i8],
    type_scale: f32,
    cols: usize,
    out: &mut [f32],
) {
    debug_assert_eq!(out.len(), cols);
    let word_src = &word_q[word_row * cols..(word_row + 1) * cols];
    let pos_src = &pos_q[pos_row * cols..(pos_row + 1) * cols];
    let type_src = &type_q[0..cols];

    #[cfg(target_arch = "x86_64")]
    {
        if is_avx2_available() {
            unsafe {
                dequant_and_sum_token_embedding_avx2(
                    word_src, word_scale, pos_src, pos_scale, type_src, type_scale, out,
                );
            }
            return;
        }
    }
    for i in 0..cols {
        out[i] = (word_src[i] as f32) * word_scale
            + (pos_src[i] as f32) * pos_scale
            + (type_src[i] as f32) * type_scale;
    }
}

/// AVX2 fused embedding dequant+sum: loads 8 i8 from each table,
/// expands i8 → i32 → f32, multiplies each by its scale, sums and
/// writes 8 fp32 outputs.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2", enable = "fma")]
unsafe fn dequant_and_sum_token_embedding_avx2(
    word_src: &[i8],
    word_scale: f32,
    pos_src: &[i8],
    pos_scale: f32,
    type_src: &[i8],
    type_scale: f32,
    out: &mut [f32],
) {
    use std::arch::x86_64::*;
    let cols = out.len();
    debug_assert_eq!(word_src.len(), cols);
    debug_assert_eq!(pos_src.len(), cols);
    debug_assert_eq!(type_src.len(), cols);
    let chunks = cols / 8;
    let tail_start = chunks * 8;

    let ws = unsafe { _mm256_set1_ps(word_scale) };
    let ps = unsafe { _mm256_set1_ps(pos_scale) };
    let ts = unsafe { _mm256_set1_ps(type_scale) };

    for c in 0..chunks {
        let off = c * 8;
        // Load 8 i8 (zero-pad upper 8 bytes), sign-extend to i32, cvt to f32.
        let w_i64 = unsafe { (word_src.as_ptr().add(off) as *const i64).read_unaligned() };
        let p_i64 = unsafe { (pos_src.as_ptr().add(off) as *const i64).read_unaligned() };
        let t_i64 = unsafe { (type_src.as_ptr().add(off) as *const i64).read_unaligned() };
        let w_i8 = unsafe { _mm_cvtsi64_si128(w_i64) };
        let p_i8 = unsafe { _mm_cvtsi64_si128(p_i64) };
        let t_i8 = unsafe { _mm_cvtsi64_si128(t_i64) };
        let w_i32 = unsafe { _mm256_cvtepi8_epi32(w_i8) };
        let p_i32 = unsafe { _mm256_cvtepi8_epi32(p_i8) };
        let t_i32 = unsafe { _mm256_cvtepi8_epi32(t_i8) };
        let w_f = unsafe { _mm256_cvtepi32_ps(w_i32) };
        let p_f = unsafe { _mm256_cvtepi32_ps(p_i32) };
        let t_f = unsafe { _mm256_cvtepi32_ps(t_i32) };
        // out = w * ws + p * ps + t * ts
        let mut acc = unsafe { _mm256_mul_ps(w_f, ws) };
        acc = unsafe { _mm256_fmadd_ps(p_f, ps, acc) };
        acc = unsafe { _mm256_fmadd_ps(t_f, ts, acc) };
        unsafe { _mm256_storeu_ps(out.as_mut_ptr().add(off), acc) };
    }

    for kk in tail_start..cols {
        out[kk] = (word_src[kk] as f32) * word_scale
            + (pos_src[kk] as f32) * pos_scale
            + (type_src[kk] as f32) * type_scale;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_matmul_2x3_3x2_col_major() {
        // A = [[1, 2, 3], [4, 5, 6]] (2x3 row-major)
        // B column-major: col 0 = [7,9,11], col 1 = [8,10,12]
        // A·B = [[58, 64], [139, 154]]
        let a: Vec<i8> = vec![1, 2, 3, 4, 5, 6];
        let b: Vec<i8> = vec![7, 9, 11, 8, 10, 12];
        let mut c = vec![0i32; 4];
        matmul_i8_scalar(2, 3, 2, &a, &b, &mut c);
        assert_eq!(c, vec![58, 64, 139, 154]);
    }

    #[test]
    fn scalar_matmul_negative_values() {
        let a: Vec<i8> = vec![-1, 2];
        let b: Vec<i8> = vec![3, -4];
        let mut c = vec![0i32; 1];
        matmul_i8_scalar(1, 2, 1, &a, &b, &mut c);
        assert_eq!(c[0], -11);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn avx2_matches_scalar() {
        if !is_avx2_available() {
            return;
        }
        let lcg = |x: u32| x.wrapping_mul(1664525).wrapping_add(1013904223);
        let mut state = 42u32;
        let mut next_i8 = || {
            state = lcg(state);
            (state.wrapping_shr(8) & 0xFF) as i8
        };
        for &(m, k, n) in &[(2, 32, 4), (3, 64, 5), (1, 97, 3), (4, 384, 8)] {
            let a: Vec<i8> = (0..m * k).map(|_| next_i8()).collect();
            let b: Vec<i8> = (0..k * n).map(|_| next_i8()).collect();
            let mut c_scalar = vec![0i32; m * n];
            let mut c_avx2 = vec![0i32; m * n];
            matmul_i8_scalar(m, k, n, &a, &b, &mut c_scalar);
            unsafe { matmul_i8_avx2(m, k, n, &a, &b, &mut c_avx2) };
            assert_eq!(c_scalar, c_avx2, "avx2 mismatch at m={m}, k={k}, n={n}");
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn quantize_avx2_matches_scalar() {
        if !is_avx2_available() {
            return;
        }
        // Compare AVX2 quantize against the scalar path. Allow ±1 LSB
        // tolerance because AVX2 rounds half-to-even while Rust's
        // `.round()` is half-away-from-zero.
        let lcg = |x: u32| x.wrapping_mul(1664525).wrapping_add(1013904223);
        let mut state = 7u32;
        let mut next_f32 = || {
            state = lcg(state);
            (state as f32 / u32::MAX as f32) * 4.0 - 2.0
        };
        for &k in &[8usize, 16, 17, 64, 97, 384, 1536] {
            let row: Vec<f32> = (0..k).map(|_| next_f32()).collect();

            // Scalar reference.
            let mut a_i8_s = vec![0i8; k];
            let mut a_u8_s = vec![0u8; k];
            let max_abs = row.iter().copied().map(f32::abs).fold(0.0f32, f32::max);
            let scale_s = (max_abs / 127.0).max(1e-12);
            let inv = 1.0 / scale_s;
            for i in 0..k {
                let q = (row[i] * inv).round().clamp(-127.0, 127.0) as i32;
                a_i8_s[i] = q as i8;
                a_u8_s[i] = (q + 128) as u8;
            }

            // AVX2.
            let mut a_i8_v = vec![0i8; k];
            let mut a_u8_v = vec![0u8; k];
            let scale_v = unsafe { quantize_row_avx2(&row, &mut a_i8_v, &mut a_u8_v) };

            assert!(
                (scale_s - scale_v).abs() < 1e-9,
                "scale mismatch at k={k}: {scale_s} vs {scale_v}"
            );
            for i in 0..k {
                let d_i = (a_i8_s[i] as i32 - a_i8_v[i] as i32).abs();
                let d_u = (a_u8_s[i] as i32 - a_u8_v[i] as i32).abs();
                assert!(
                    d_i <= 1,
                    "i8 mismatch at k={k}, i={i}: scalar={} avx2={}",
                    a_i8_s[i],
                    a_i8_v[i]
                );
                assert!(
                    d_u <= 1,
                    "u8 mismatch at k={k}, i={i}: scalar={} avx2={}",
                    a_u8_s[i],
                    a_u8_v[i]
                );
            }
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn vnni_matches_scalar() {
        if !is_avxvnni_available() {
            eprintln!("AVX-VNNI not available; skipping");
            return;
        }
        let lcg = |x: u32| x.wrapping_mul(1664525).wrapping_add(1013904223);
        let mut state = 12345u32;
        let mut next_i8 = || {
            state = lcg(state);
            (state.wrapping_shr(8) & 0xFF) as i8
        };
        // Mix of even/odd m and various n alignments to exercise both
        // the 2×4 main path and the odd-row + j-tail branches.
        for &(m, k, n) in &[
            (2, 32, 4),
            (3, 64, 5),
            (1, 97, 3),
            (4, 384, 8),
            (3, 384, 12),
            (5, 384, 9),
            (7, 384, 13),
            (13, 384, 384),
        ] {
            let a_i8: Vec<i8> = (0..m * k).map(|_| next_i8()).collect();
            let a_u8: Vec<u8> = a_i8.iter().map(|&v| (v as i32 + 128) as u8).collect();
            let b: Vec<i8> = (0..k * n).map(|_| next_i8()).collect();

            // Compute per-column sums of B.
            let mut col_sums = vec![0i32; n];
            for j in 0..n {
                let mut s = 0i32;
                for kk in 0..k {
                    s += b[j * k + kk] as i32;
                }
                col_sums[j] = s;
            }

            let mut c_scalar = vec![0i32; m * n];
            let mut c_vnni = vec![0i32; m * n];
            matmul_i8_scalar(m, k, n, &a_i8, &b, &mut c_scalar);
            unsafe { matmul_i8_vnni(m, k, n, &a_u8, &b, &col_sums, &mut c_vnni) };
            assert_eq!(c_scalar, c_vnni, "vnni mismatch at m={m}, k={k}, n={n}");
        }
    }
}
