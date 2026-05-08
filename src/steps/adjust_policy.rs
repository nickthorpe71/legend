use crate::types::{Intent, Policy};

/// Step 2 of the tick pipeline. Map the 4-dim `Intent` from Step 1 onto
/// the substrate knobs via the §10.6 formulas, returning a per-tick
/// Policy that Steps 3–13 see. Pure scalar arithmetic — no model, no
/// allocation beyond the Policy clone.
///
/// Each input field on `base` is the rest-state value (treated as
/// `base_*` in the formulas); the corresponding field on the returned
/// Policy is the intent-modulated scalar. Every other field is copied
/// through unchanged.
///
/// The base Policy on the Hypergraph is the inter-tick rest state; only
/// PFC writes it. Tick-internal subroutines see this adjusted view and
/// never the rest state directly.
pub fn adjust_policy(intent: &Intent, base: &Policy) -> Policy {
    let mut p = base.clone();

    // conviction × (1 - curiosity) → speaker certainty separated from
    // "speaker is asking." A confident question still writes new
    // content low-confidence because the speaker isn't asserting it.
    p.default_conf = base.default_conf * intent.conviction * (1.0 - 0.7 * intent.curiosity);

    // DA + NE encoding boost: surprise and emotional intensity both
    // raise the per-tick salience multiplier additively.
    p.salience_multiplier = base.salience_multiplier + intent.arousal + intent.prediction_error;

    // Both contradiction and confident assertion warrant tighter
    // routing — don't blur entities during corrections or identity
    // claims. Brainstorming (low both) loosens routing.
    p.leaf_vigilance =
        base.leaf_vigilance + 0.20 * intent.prediction_error + 0.20 * intent.conviction;

    // Questions traverse paths but reinforce them more lightly than
    // statements; arousal still amplifies when present.
    p.hebbian_rate =
        base.hebbian_rate * (1.0 - 0.5 * intent.curiosity) * (1.0 + 0.3 * intent.arousal);

    // High-PE inputs are exactly when prior beliefs need revisiting;
    // low-PE inputs leave the cache alone.
    p.supersession_threshold = base.supersession_threshold * (1.0 - intent.prediction_error);

    p
}
