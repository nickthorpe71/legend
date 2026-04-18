/// Smooth transfer functions for bounded memory signals.
///
/// Hard clamps erase differences once a raw signal crosses the ceiling. These
/// helpers use saturating curves instead: strong finite inputs still preserve
/// rank, while extreme values approach the ceiling gradually.

/// Normalize a non-negative signal into `[floor, ceiling)` using a Hill curve.
///
/// `midpoint` is the raw value that maps halfway between `floor` and `ceiling`.
/// `steepness` controls how sharply the curve rises around the midpoint.
pub fn normalize_positive_signal(
    raw: f32,
    floor: f32,
    ceiling: f32,
    midpoint: f32,
    steepness: f32,
) -> f32 {
    if !raw.is_finite() || !floor.is_finite() || !ceiling.is_finite() {
        return floor;
    }

    let floor = floor.min(ceiling);
    let span = ceiling - floor;
    if span <= 0.0 {
        return floor;
    }

    let raw = raw.max(0.0);
    if raw == 0.0 {
        return floor;
    }

    let midpoint = midpoint.max(f32::EPSILON);
    let steepness = if steepness.is_finite() {
        steepness.max(f32::EPSILON)
    } else {
        1.0
    };

    let powered_raw = raw.powf(steepness);
    let powered_midpoint = midpoint.powf(steepness);

    floor + span * powered_raw / (powered_raw + powered_midpoint)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_positive_signal_floor_for_zero_or_negative() {
        assert_eq!(normalize_positive_signal(0.0, 0.05, 1.0, 0.45, 1.4), 0.05);
        assert_eq!(normalize_positive_signal(-10.0, 0.05, 1.0, 0.45, 1.4), 0.05);
    }

    #[test]
    fn test_normalize_positive_signal_monotonic() {
        let a = normalize_positive_signal(0.1, 0.05, 1.0, 0.45, 1.4);
        let b = normalize_positive_signal(0.5, 0.05, 1.0, 0.45, 1.4);
        let c = normalize_positive_signal(2.0, 0.05, 1.0, 0.45, 1.4);

        assert!(a < b, "{a} should be less than {b}");
        assert!(b < c, "{b} should be less than {c}");
    }

    #[test]
    fn test_normalize_positive_signal_bounded_without_hard_ceiling() {
        let high = normalize_positive_signal(5.0, 0.05, 1.0, 0.45, 1.4);
        let higher = normalize_positive_signal(10.0, 0.05, 1.0, 0.45, 1.4);

        assert!(high < 1.0, "{high} should stay below ceiling");
        assert!(higher < 1.0, "{higher} should stay below ceiling");
        assert!(
            higher > high,
            "{higher} should preserve high-end rank over {high}"
        );
    }

    #[test]
    fn test_normalize_positive_signal_midpoint() {
        let s = normalize_positive_signal(0.45, 0.05, 1.0, 0.45, 1.4);
        assert!(
            (s - 0.525).abs() < 0.001,
            "midpoint should map halfway through span, got {s}"
        );
    }
}
