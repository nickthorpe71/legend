/// Amygdala — emotional valence computation.
///
/// Computes a bipolar emotional signal from text: negative for threats (bugs,
/// crashes, security issues), positive for rewards (shipped, fixed, success).
/// Urgency keywords amplify the magnitude toward ±1.0.
///
/// The valence value persists on memory entries alongside salience but decays
/// at half the hippocampal rate, modeling how emotionally charged memories
/// resist forgetting.

use super::keyword_cache::KeywordCache;

/// Compute emotional valence for a text fragment.
///
/// Returns a value in [-1.0, 1.0]:
/// - Negative → threat/pain signal (bugs, crashes, security)
/// - Positive → reward signal (shipped, fixed, success)
/// - Zero → neutral
///
/// Urgency keywords (blocker, critical, P0) amplify magnitude toward extremes.
pub fn compute_emotional_valence(text: &str, kw: &KeywordCache) -> f32 {
    let lowered = text.to_lowercase();
    let mut valence: f32 = 0.0;

    // Accumulate negative valence weights from matched keywords
    for (keyword, weight) in &kw.negative_valence {
        if lowered.contains(keyword.as_str()) {
            valence += weight;
        }
    }

    // Accumulate positive valence weights
    for (keyword, weight) in &kw.positive_valence {
        if lowered.contains(keyword.as_str()) {
            valence += weight;
        }
    }

    // Urgency amplifier: push magnitude toward ±1.0
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

    #[test]
    fn test_negative_valence_bug_report() {
        let v = compute_emotional_valence("BUG: server crashes on null input", &kw());
        assert!(v < -0.3, "bug+crash should be strongly negative, got {}", v);
    }

    #[test]
    fn test_negative_valence_security() {
        let v = compute_emotional_valence("Found a SQL injection vulnerability in login", &kw());
        assert!(v < -0.3, "security issue should be negative, got {}", v);
    }

    #[test]
    fn test_negative_valence_data_loss() {
        let v = compute_emotional_valence("data loss in production after failed migration", &kw());
        assert!(v < -0.5, "data loss should be strongly negative, got {}", v);
    }

    #[test]
    fn test_positive_valence_shipped() {
        let v = compute_emotional_valence("SHIPPED: v2.0 released successfully", &kw());
        assert!(v > 0.3, "shipping should be positive, got {}", v);
    }

    #[test]
    fn test_positive_valence_fixed() {
        let v = compute_emotional_valence("Fixed the authentication bug, all tests pass", &kw());
        assert!(v > 0.0, "fix+passing should be positive, got {}", v);
    }

    #[test]
    fn test_neutral_text() {
        let v = compute_emotional_valence("updated the documentation formatting", &kw());
        assert!(
            v.abs() < 0.3,
            "neutral text should be near zero, got {}",
            v
        );
    }

    #[test]
    fn test_urgency_amplifies_negative() {
        let without = compute_emotional_valence("there is a bug in the system", &kw());
        let with = compute_emotional_valence("BLOCKER: critical bug in the system", &kw());
        assert!(
            with < without,
            "urgency should amplify negative: {} vs {}",
            with,
            without
        );
    }

    #[test]
    fn test_urgency_amplifies_positive() {
        let without = compute_emotional_valence("shipped the update", &kw());
        let with = compute_emotional_valence("urgent shipped the update", &kw());
        // Urgency keyword should amplify the positive signal
        assert!(
            with > without,
            "urgency should amplify positive: with={} vs without={}",
            with,
            without
        );
    }

    #[test]
    fn test_valence_clamped() {
        // Pile on every negative keyword we can
        let v = compute_emotional_valence(
            "BLOCKER critical P0 urgent crash panic segfault fatal data loss corruption vulnerability exploit incident outage",
            &kw(),
        );
        assert!(v >= -1.0 && v <= 1.0, "should be clamped, got {}", v);
    }

    #[test]
    fn test_mixed_valence() {
        // Both positive and negative signals — should partially cancel
        let v = compute_emotional_valence("Fixed the crash", &kw());
        // Has negative (crash: -0.5) and positive (fixed: +0.4) — should partially cancel
        let pure_neg = compute_emotional_valence("the server crashed", &kw());
        assert!(
            v > pure_neg,
            "mixed signals should be less negative than pure negative: {} vs {}",
            v,
            pure_neg
        );
    }

    #[test]
    fn test_empty_text() {
        let v = compute_emotional_valence("", &kw());
        assert_eq!(v, 0.0);
    }
}
