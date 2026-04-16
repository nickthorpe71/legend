/// Neurochemical modulation system — Phase A.
///
/// Models 6 neurochemicals with different triggers, decay profiles, and
/// downstream effects on memory dynamics. Global levels are updated each tick
/// and consumed via `EffectiveChemistry` computed multipliers.
///
/// | Chemical          | Role                                     | Half-life |
/// |-------------------|------------------------------------------|-----------|
/// | Norepinephrine    | Encoding amplifier, attention gate       | ~3 ticks  |
/// | Cortisol          | Consolidation pressure, Yerkes-Dodson    | ~20 ticks |
/// | Dopamine          | Reinforcement, Hebbian boost             | ~8 ticks  |
/// | Serotonin         | Baseline stability, decay modulation     | ~50 ticks |
/// | Acetylcholine     | Encoding/retrieval switch, novelty       | ~6 ticks  |
/// | Endocannabinoid   | Active forgetting, stress recovery       | ~15 ticks |
use serde::{Deserialize, Serialize};

// --- Decay half-lives (in ticks) ---
const NE_HALF_LIFE: f32 = 3.0;
const CORTISOL_HALF_LIFE: f32 = 20.0;
const DA_HALF_LIFE: f32 = 8.0;
const SEROTONIN_HALF_LIFE: f32 = 50.0;
const ACH_HALF_LIFE: f32 = 6.0;
const ECB_HALF_LIFE: f32 = 15.0;

// --- Spike magnitudes ---
pub const NE_SALIENCE_SPIKE: f32 = 0.3;
pub const NE_CONTEXT_SWITCH_SPIKE: f32 = 0.5;
/// Smaller NE spike for negative-valence threat arousal (separate from salience spike).
pub const NE_THREAT_SPIKE: f32 = 0.15;
pub const CORTISOL_NEGATIVE_SPIKE: f32 = 0.2;
pub const DA_POSITIVE_SPIKE: f32 = 0.2;
pub const DA_RETRIEVAL_SPIKE: f32 = 0.1;
pub const ACH_NOVELTY_SPIKE: f32 = 0.3;
pub const ECB_ROUTINE_SPIKE: f32 = 0.1;
pub const ECB_CORTISOL_RECOVERY: f32 = 0.3;

// --- Hippocampal overload defense ---
/// L2 fill ratio at which capacity-proportional cortisol begins rising.
pub const CAPACITY_STRESS_ONSET: f32 = 0.75;
/// Maximum cortisol spike per tick at full L2 capacity (scales linearly with fill ratio above onset).
pub const CORTISOL_CAPACITY_SPIKE: f32 = 0.15;

/// State-dependent retrieval bonus: max similarity boost when chemistry matches encoding.
pub const STATE_DEPENDENT_BONUS: f32 = 0.05;

/// Serotonin homeostasis pull strength per tick.
const SEROTONIN_HOMEOSTASIS_RATE: f32 = 0.02;
/// Serotonin baseline target.
const SEROTONIN_BASELINE: f32 = 0.5;

/// Global neurochemical state — systemic levels that modulate all brain functions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Neurochemistry {
    pub norepinephrine: f32,
    pub cortisol: f32,
    pub dopamine: f32,
    pub serotonin: f32,
    pub acetylcholine: f32,
    pub endocannabinoid: f32,
}

impl Default for Neurochemistry {
    fn default() -> Self {
        Self {
            norepinephrine: 0.0,
            cortisol: 0.0,
            dopamine: 0.0,
            serotonin: SEROTONIN_BASELINE,
            acetylcholine: 0.0,
            endocannabinoid: 0.0,
        }
    }
}

/// Computed once per tick — effective multipliers after chemical interactions.
/// Downstream functions consume these instead of reading raw levels.
#[allow(dead_code)]
pub struct EffectiveChemistry {
    /// NE × ACh synergy — scales salience/encoding strength.
    /// (Inlined per-chunk in tick_impl for live-chemistry responsiveness.)
    pub encoding_gain: f32,
    /// Cortisol-driven consolidation urgency (replaces recent_valence_sum).
    pub consolidation_pressure: f32,
    /// Yerkes-Dodson retrieval quality (inverted-U of cortisol, ACh mode).
    pub retrieval_quality: f32,
    /// DA × (1 - high cortisol) — Hebbian/spaced-rep gain.
    pub reinforcement_gain: f32,
    /// 5-HT baseline × eCB modulation — global decay rate multiplier.
    pub decay_rate_mod: f32,
    /// eCB level × (1 - DA protection) — pruning aggressiveness.
    pub pruning_pressure: f32,
}

// ---------------------------------------------------------------------------
// Decay
// ---------------------------------------------------------------------------

/// Exponential decay for a single chemical: loses half its value every `half_life` ticks.
#[inline]
fn decay_chemical(level: f32, half_life: f32) -> f32 {
    level * 0.5_f32.powf(1.0 / half_life)
}

/// Apply one tick of exponential decay to all 6 chemicals.
pub fn apply_decay(chem: &mut Neurochemistry) {
    chem.norepinephrine = decay_chemical(chem.norepinephrine, NE_HALF_LIFE);
    chem.cortisol = decay_chemical(chem.cortisol, CORTISOL_HALF_LIFE);
    chem.dopamine = decay_chemical(chem.dopamine, DA_HALF_LIFE);
    chem.serotonin = decay_chemical(chem.serotonin, SEROTONIN_HALF_LIFE);
    chem.acetylcholine = decay_chemical(chem.acetylcholine, ACH_HALF_LIFE);
    chem.endocannabinoid = decay_chemical(chem.endocannabinoid, ECB_HALF_LIFE);
}

/// Pull serotonin toward its homeostatic baseline (0.5).
pub fn update_serotonin_homeostasis(chem: &mut Neurochemistry) {
    let delta = SEROTONIN_BASELINE - chem.serotonin;
    chem.serotonin += delta * SEROTONIN_HOMEOSTASIS_RATE;
}

// ---------------------------------------------------------------------------
// Yerkes-Dodson inverted-U
// ---------------------------------------------------------------------------

/// Inverted-U curve: peak performance at moderate cortisol (~0.4), impaired at extremes.
/// Returns 0.0–1.0.
pub fn yerkes_dodson(cortisol: f32) -> f32 {
    let mean = 0.4_f32;
    let sigma = 0.3_f32;
    let exponent = -((cortisol - mean).powi(2)) / (2.0 * sigma * sigma);
    exponent.exp()
}

// ---------------------------------------------------------------------------
// Effective chemistry (pairwise interactions)
// ---------------------------------------------------------------------------

/// Compute downstream effective multipliers from raw chemical levels.
pub fn compute_effective(chem: &Neurochemistry) -> EffectiveChemistry {
    // NE × ACh synergy for encoding
    let encoding_gain = 1.0 + chem.norepinephrine * 0.5 + chem.acetylcholine * 0.3;

    // Cortisol drives consolidation pressure (replaces recent_valence_sum)
    let consolidation_pressure = chem.cortisol * 2.0 + chem.norepinephrine * 0.5;

    // Yerkes-Dodson retrieval quality modulated by ACh
    let retrieval_quality = yerkes_dodson(chem.cortisol) * (1.0 + chem.acetylcholine * 0.2);

    // DA reinforcement gain, attenuated by high cortisol (stress impairs learning)
    let cortisol_attenuation = (1.0 - chem.cortisol * 0.5).max(0.3);
    let reinforcement_gain = 1.0 + chem.dopamine * 0.5 * cortisol_attenuation;

    // 5-HT stabilizes decay; eCB accelerates forgetting
    let serotonin_stability = 0.8 + chem.serotonin * 0.4; // 0.8-1.2 range
    let ecb_acceleration = 1.0 + chem.endocannabinoid * 0.3;
    let decay_rate_mod = ecb_acceleration / serotonin_stability;

    // eCB pruning, protected by DA (important memories resist pruning)
    let da_protection = (1.0 - chem.dopamine * 0.5).max(0.2);
    let pruning_pressure = chem.endocannabinoid * da_protection;

    EffectiveChemistry {
        encoding_gain,
        consolidation_pressure,
        retrieval_quality,
        reinforcement_gain,
        decay_rate_mod,
        pruning_pressure,
    }
}

// ---------------------------------------------------------------------------
// Chemical stamps — Phase B: encoding-time chemistry snapshot
// ---------------------------------------------------------------------------

/// Snapshot of key neurochemical levels at the moment a memory is encoded.
/// Stored on each L2 entry to enable state-dependent retrieval, NE-protected
/// emotional decay, and DA-enhanced reinforcement.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ChemicalStamp {
    pub ne_at_encoding: f32,
    pub cortisol_at_encoding: f32,
    pub da_at_encoding: f32,
    pub ach_at_encoding: f32,
}

impl Default for ChemicalStamp {
    fn default() -> Self {
        Self {
            ne_at_encoding: 0.0,
            cortisol_at_encoding: 0.0,
            da_at_encoding: 0.0,
            ach_at_encoding: 0.0,
        }
    }
}

/// Snapshot current chemistry levels into a stamp for encoding.
pub fn stamp_from(chem: &Neurochemistry) -> ChemicalStamp {
    ChemicalStamp {
        ne_at_encoding: chem.norepinephrine,
        cortisol_at_encoding: chem.cortisol,
        da_at_encoding: chem.dopamine,
        ach_at_encoding: chem.acetylcholine,
    }
}

/// Cosine similarity between a 4-dim stamp vector and current chemistry levels.
/// Returns 0.0–1.0 (clamped). Zero vectors yield 0.0.
pub fn chemical_state_match(stamp: &ChemicalStamp, current: &Neurochemistry) -> f32 {
    let a = [
        stamp.ne_at_encoding,
        stamp.cortisol_at_encoding,
        stamp.da_at_encoding,
        stamp.ach_at_encoding,
    ];
    let b = [
        current.norepinephrine,
        current.cortisol,
        current.dopamine,
        current.acetylcholine,
    ];

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let mag_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let mag_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if mag_a < f32::EPSILON || mag_b < f32::EPSILON {
        return 0.0;
    }

    (dot / (mag_a * mag_b)).clamp(0.0, 1.0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_levels() {
        let c = Neurochemistry::default();
        assert_eq!(c.norepinephrine, 0.0);
        assert_eq!(c.cortisol, 0.0);
        assert_eq!(c.dopamine, 0.0);
        assert_eq!(c.serotonin, 0.5);
        assert_eq!(c.acetylcholine, 0.0);
        assert_eq!(c.endocannabinoid, 0.0);
    }

    #[test]
    fn test_chemical_decay_half_lives() {
        // Each chemical should reach ~0.5 at its half-life number of ticks
        let test_cases: Vec<(&str, f32, fn(&Neurochemistry) -> f32)> = vec![
            ("NE", NE_HALF_LIFE, |c| c.norepinephrine),
            ("Cortisol", CORTISOL_HALF_LIFE, |c| c.cortisol),
            ("DA", DA_HALF_LIFE, |c| c.dopamine),
            ("5-HT", SEROTONIN_HALF_LIFE, |c| c.serotonin),
            ("ACh", ACH_HALF_LIFE, |c| c.acetylcholine),
            ("eCB", ECB_HALF_LIFE, |c| c.endocannabinoid),
        ];

        for (name, half_life, getter) in &test_cases {
            let mut chem = Neurochemistry {
                norepinephrine: 1.0,
                cortisol: 1.0,
                dopamine: 1.0,
                serotonin: 1.0,
                acetylcholine: 1.0,
                endocannabinoid: 1.0,
            };

            for _ in 0..*half_life as u32 {
                apply_decay(&mut chem);
            }

            let level = getter(&chem);
            assert!(
                (level - 0.5).abs() < 0.05,
                "{name} should be ~0.5 after {half_life} ticks, got {level:.4}"
            );
        }
    }

    #[test]
    fn test_serotonin_homeostasis() {
        let mut chem = Neurochemistry::default();
        chem.serotonin = 0.0;

        // Apply homeostasis many times — should drift toward 0.5
        for _ in 0..200 {
            update_serotonin_homeostasis(&mut chem);
        }

        assert!(
            (chem.serotonin - 0.5).abs() < 0.05,
            "5-HT should drift toward 0.5, got {}",
            chem.serotonin
        );
    }

    #[test]
    fn test_yerkes_dodson_inverted_u() {
        // Peak at moderate cortisol (~0.4)
        let peak = yerkes_dodson(0.4);
        let low = yerkes_dodson(0.0);
        let high = yerkes_dodson(1.0);

        assert!(
            peak > low,
            "peak ({peak}) should exceed low cortisol ({low})"
        );
        assert!(
            peak > high,
            "peak ({peak}) should exceed high cortisol ({high})"
        );
        assert!(
            (peak - 1.0).abs() < 0.01,
            "peak should be ~1.0, got {peak}"
        );
    }

    #[test]
    fn test_compute_effective_baseline() {
        let chem = Neurochemistry::default();
        let eff = compute_effective(&chem);

        // At baseline: encoding_gain ~1.0, reinforcement_gain ~1.0
        assert!(
            (eff.encoding_gain - 1.0).abs() < 0.01,
            "baseline encoding_gain should be ~1.0, got {}",
            eff.encoding_gain
        );
        assert!(
            (eff.reinforcement_gain - 1.0).abs() < 0.01,
            "baseline reinforcement_gain should be ~1.0, got {}",
            eff.reinforcement_gain
        );
        // Consolidation pressure should be near 0 at baseline
        assert!(
            eff.consolidation_pressure < 0.3,
            "baseline consolidation_pressure should be low, got {}",
            eff.consolidation_pressure
        );
    }

    #[test]
    fn test_high_cortisol_attenuates_reinforcement() {
        let mut chem = Neurochemistry::default();
        chem.dopamine = 0.5;
        chem.cortisol = 0.0;
        let eff_low_stress = compute_effective(&chem);

        chem.cortisol = 0.8;
        let eff_high_stress = compute_effective(&chem);

        assert!(
            eff_high_stress.reinforcement_gain < eff_low_stress.reinforcement_gain,
            "high cortisol should attenuate reinforcement: {} vs {}",
            eff_high_stress.reinforcement_gain,
            eff_low_stress.reinforcement_gain
        );
    }

    #[test]
    fn test_ecb_cortisol_recovery() {
        let mut chem = Neurochemistry::default();
        chem.cortisol = 0.8;

        // Simulate eCB recovery from high cortisol
        if chem.cortisol > 0.5 {
            chem.endocannabinoid =
                (chem.endocannabinoid + (chem.cortisol - 0.5) * ECB_CORTISOL_RECOVERY).min(1.0);
        }

        assert!(
            chem.endocannabinoid > 0.0,
            "eCB should rise in response to high cortisol, got {}",
            chem.endocannabinoid
        );
    }

    #[test]
    fn test_da_protects_against_pruning() {
        let mut chem = Neurochemistry::default();
        chem.endocannabinoid = 0.5;
        chem.dopamine = 0.0;
        let eff_no_da = compute_effective(&chem);

        chem.dopamine = 0.8;
        let eff_with_da = compute_effective(&chem);

        assert!(
            eff_with_da.pruning_pressure < eff_no_da.pruning_pressure,
            "DA should protect against pruning: {} vs {}",
            eff_with_da.pruning_pressure,
            eff_no_da.pruning_pressure
        );
    }

    #[test]
    fn test_chemistry_serialization() {
        let chem = Neurochemistry {
            norepinephrine: 0.3,
            cortisol: 0.5,
            dopamine: 0.7,
            serotonin: 0.4,
            acetylcholine: 0.2,
            endocannabinoid: 0.1,
        };

        let serialized = serde_json::to_string(&chem).unwrap();
        let deserialized: Neurochemistry = serde_json::from_str(&serialized).unwrap();

        assert!((deserialized.norepinephrine - 0.3).abs() < f32::EPSILON);
        assert!((deserialized.cortisol - 0.5).abs() < f32::EPSILON);
        assert!((deserialized.dopamine - 0.7).abs() < f32::EPSILON);
        assert!((deserialized.serotonin - 0.4).abs() < f32::EPSILON);
        assert!((deserialized.acetylcholine - 0.2).abs() < f32::EPSILON);
        assert!((deserialized.endocannabinoid - 0.1).abs() < f32::EPSILON);
    }

    // --- Phase B: Chemical stamp tests ---

    #[test]
    fn test_stamp_from_captures_levels() {
        let chem = Neurochemistry {
            norepinephrine: 0.4,
            cortisol: 0.2,
            dopamine: 0.6,
            acetylcholine: 0.3,
            ..Default::default()
        };
        let stamp = stamp_from(&chem);
        assert!((stamp.ne_at_encoding - 0.4).abs() < f32::EPSILON);
        assert!((stamp.cortisol_at_encoding - 0.2).abs() < f32::EPSILON);
        assert!((stamp.da_at_encoding - 0.6).abs() < f32::EPSILON);
        assert!((stamp.ach_at_encoding - 0.3).abs() < f32::EPSILON);
    }

    #[test]
    fn test_stamp_defaults_to_zero() {
        // Simulate deserializing an old entry without a stamp
        let stamp: ChemicalStamp = serde_json::from_str("{}").unwrap();
        assert_eq!(stamp.ne_at_encoding, 0.0);
        assert_eq!(stamp.cortisol_at_encoding, 0.0);
        assert_eq!(stamp.da_at_encoding, 0.0);
        assert_eq!(stamp.ach_at_encoding, 0.0);
    }

    #[test]
    fn test_chemical_state_match_identical() {
        let chem = Neurochemistry {
            norepinephrine: 0.5,
            cortisol: 0.3,
            dopamine: 0.4,
            acetylcholine: 0.2,
            ..Default::default()
        };
        let stamp = stamp_from(&chem);
        let sim = chemical_state_match(&stamp, &chem);
        assert!(
            (sim - 1.0).abs() < 0.01,
            "identical chemistry should yield ~1.0, got {sim}"
        );
    }

    #[test]
    fn test_chemical_state_match_zero_vectors() {
        let stamp = ChemicalStamp::default();
        let chem = Neurochemistry::default(); // all zeros except serotonin (ignored in stamp)
        let sim = chemical_state_match(&stamp, &chem);
        assert_eq!(sim, 0.0, "zero vectors should yield 0.0");
    }

    #[test]
    fn test_chemical_state_match_orthogonal() {
        let stamp = ChemicalStamp {
            ne_at_encoding: 1.0,
            cortisol_at_encoding: 0.0,
            da_at_encoding: 0.0,
            ach_at_encoding: 0.0,
        };
        let chem = Neurochemistry {
            norepinephrine: 0.0,
            cortisol: 1.0,
            dopamine: 0.0,
            acetylcholine: 0.0,
            ..Default::default()
        };
        let sim = chemical_state_match(&stamp, &chem);
        assert!(
            sim < 0.01,
            "orthogonal chemistry should yield ~0.0, got {sim}"
        );
    }

    #[test]
    fn test_chemical_stamp_serialization() {
        let stamp = ChemicalStamp {
            ne_at_encoding: 0.3,
            cortisol_at_encoding: 0.5,
            da_at_encoding: 0.7,
            ach_at_encoding: 0.2,
        };
        let serialized = serde_json::to_string(&stamp).unwrap();
        let deserialized: ChemicalStamp = serde_json::from_str(&serialized).unwrap();
        assert!((deserialized.ne_at_encoding - 0.3).abs() < f32::EPSILON);
        assert!((deserialized.da_at_encoding - 0.7).abs() < f32::EPSILON);
    }
}
