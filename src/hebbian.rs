//! Bounded Hebbian operators (Oja-rule-derived) used wherever
//! `MemoryStats` activation / salience / co-activation weights need
//! to update without leaving the closed interval `[0, 1]`.
//!
//! Two operators, both expecting `rate ∈ [0, 1]` and `x ∈ [0, 1]`:
//!
//! - [`bounded_hebbian_bump`] — moves `x` toward 1.0; asymptotes from
//!   below. `bump(x, 0.0) == x`, `bump(x, 1.0) == 1.0`.
//! - [`bounded_hebbian_decay`] — moves `x` toward 0.0; asymptotes from
//!   above. `decay(x, 0.0) == x`, `decay(x, 1.0) == 0.0`.
//!
//! Per `new_foundation.md` §14.9. `hebbian_and_salience` reinforces via `_bump`;
//! `focus_radius_decay`'s focus-radius decay walks via `_decay`.
//!
//! Callers are responsible for keeping `rate` and `x` in range. Out-
//! of-range inputs are clamped on the way out so a stray epsilon never
//! propagates a value outside `[0, 1]` into the substrate.

/// `x + rate * (1 - x)` clamped to `[0, 1]`. Monotonic in `x` and in
/// `rate`. At `rate = 0`, no-op; at `rate = 1`, jumps straight to 1.0.
pub fn bounded_hebbian_bump(x: f32, rate: f32) -> f32 {
    (x + rate * (1.0 - x)).clamp(0.0, 1.0)
}

/// `x * (1 - rate)` clamped to `[0, 1]`. Monotonic in `x`; inversely
/// monotonic in `rate`. At `rate = 0`, no-op; at `rate = 1`, drops to 0.
pub fn bounded_hebbian_decay(x: f32, rate: f32) -> f32 {
    (x * (1.0 - rate)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bump_rate_zero_is_noop() {
        assert_eq!(bounded_hebbian_bump(0.3, 0.0), 0.3);
        assert_eq!(bounded_hebbian_bump(0.0, 0.0), 0.0);
        assert_eq!(bounded_hebbian_bump(1.0, 0.0), 1.0);
    }

    #[test]
    fn bump_rate_one_lands_at_one() {
        assert_eq!(bounded_hebbian_bump(0.0, 1.0), 1.0);
        assert_eq!(bounded_hebbian_bump(0.5, 1.0), 1.0);
        assert_eq!(bounded_hebbian_bump(1.0, 1.0), 1.0);
    }

    #[test]
    fn bump_is_monotonically_increasing_in_x() {
        let rate = 0.3;
        let mut last = bounded_hebbian_bump(0.0, rate);
        for step in 1..=10 {
            let x = step as f32 / 10.0;
            let cur = bounded_hebbian_bump(x, rate);
            assert!(
                cur >= last,
                "expected monotonic increase: x={x} cur={cur} last={last}"
            );
            last = cur;
        }
    }

    #[test]
    fn bump_asymptotes_to_one() {
        // 100 bumps at rate 0.1 should approach 1.0 from below.
        let mut x: f32 = 0.0;
        for _ in 0..100 {
            x = bounded_hebbian_bump(x, 0.1);
        }
        assert!(x > 0.999, "expected x ≈ 1.0 after 100 bumps; got {x}");
        assert!(x <= 1.0, "must never exceed 1.0");
    }

    #[test]
    fn bump_never_decreases_x_for_positive_rate() {
        for x_step in 0..=10 {
            for r_step in 0..=10 {
                let x = x_step as f32 / 10.0;
                let r = r_step as f32 / 10.0;
                let out = bounded_hebbian_bump(x, r);
                assert!(
                    out >= x - 1e-6,
                    "bump should not decrease x: x={x} r={r} out={out}",
                );
            }
        }
    }

    #[test]
    fn decay_rate_zero_is_noop() {
        assert_eq!(bounded_hebbian_decay(0.7, 0.0), 0.7);
        assert_eq!(bounded_hebbian_decay(1.0, 0.0), 1.0);
    }

    #[test]
    fn decay_rate_one_lands_at_zero() {
        assert_eq!(bounded_hebbian_decay(1.0, 1.0), 0.0);
        assert_eq!(bounded_hebbian_decay(0.5, 1.0), 0.0);
    }

    #[test]
    fn decay_asymptotes_to_zero() {
        let mut x: f32 = 1.0;
        for _ in 0..100 {
            x = bounded_hebbian_decay(x, 0.1);
        }
        assert!(x < 1e-3, "expected x ≈ 0 after 100 decays; got {x}");
        assert!(x >= 0.0, "must never go below 0");
    }

    #[test]
    fn decay_never_increases_x_for_positive_rate() {
        for x_step in 0..=10 {
            for r_step in 0..=10 {
                let x = x_step as f32 / 10.0;
                let r = r_step as f32 / 10.0;
                let out = bounded_hebbian_decay(x, r);
                assert!(
                    out <= x + 1e-6,
                    "decay should not increase x: x={x} r={r} out={out}",
                );
            }
        }
    }

    #[test]
    fn bump_and_decay_stay_in_unit_interval() {
        // Stress: walk a 2D grid of (x, rate), verify both operators
        // stay in [0, 1] even at extreme inputs.
        for x_step in 0..=20 {
            for r_step in 0..=20 {
                let x = x_step as f32 / 20.0;
                let r = r_step as f32 / 20.0;
                let b = bounded_hebbian_bump(x, r);
                let d = bounded_hebbian_decay(x, r);
                assert!((0.0..=1.0).contains(&b), "bump out of range: {b}");
                assert!((0.0..=1.0).contains(&d), "decay out of range: {d}");
            }
        }
    }

    #[test]
    fn bump_clamps_negative_rate_safely() {
        // Defensive: negative rate would otherwise reduce x; clamp
        // pulls back into range. Don't test the math — just that
        // we never escape [0, 1].
        let out = bounded_hebbian_bump(0.5, -0.5);
        assert!((0.0..=1.0).contains(&out));
    }
}
