//! Audit the current intent classifiers — reveals what they're keying on.
//!
//! Run: `cargo run --release --example audit_classifiers 2>/dev/null`
//!
//! Three diagnostics:
//!   1. Inter-classifier weight cosines (4×4 matrix). Tells us if pairs of
//!      classifiers have correlated weight vectors — partial confounding.
//!   2. Top-K activating phrases per classifier, drawn from the full seed
//!      pack. Reveals what each classifier "thinks" causes high-of-itself.
//!      If conviction's top activators are all from prediction_error.high,
//!      conviction is hijacking that signal.
//!   3. Paraphrase invariance. Hand-crafted intent-preserving rewrites are
//!      scored and the score delta is reported. Reveals how surface-form
//!      sensitive each classifier is.

#[path = "shared/mod.rs"]
mod shared;

use legend::intent_classifiers::{
    Classifier, IntentClassifiers, featurize, load_intent_classifiers,
};
use legend::math::{cosine, dot, sigmoid};
use shared::{SeedPack, dims, load_seed_pack};
use std::path::Path;

const DIMS: [&str; 4] = ["conviction", "prediction_error", "arousal", "curiosity"];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pack = load_seed_pack(Path::new("seed_pack.yaml"))?;
    let clfs = load_intent_classifiers();

    diagnostic_1_weight_cosines(&clfs);
    diagnostic_2_top_activators(&clfs, &pack);
    diagnostic_3_paraphrase_invariance(&clfs);

    Ok(())
}

// ── Diagnostic 1 ────────────────────────────────────────────────────────

fn diagnostic_1_weight_cosines(clfs: &IntentClassifiers) {
    println!("\n=== 1. Inter-classifier weight-vector cosines ===");
    println!("(High off-diagonal = classifiers are picking up correlated signals)");
    println!();

    let weights: [&[f32]; 4] = [
        &clfs.conviction.weights,
        &clfs.prediction_error.weights,
        &clfs.arousal.weights,
        &clfs.curiosity.weights,
    ];

    print!("                  ");
    for d in &DIMS {
        print!("{:>16}  ", d);
    }
    println!();

    for (i, &name) in DIMS.iter().enumerate() {
        print!("{:>16}  ", name);
        for j in 0..4 {
            let cos = cosine(weights[i], weights[j]);
            print!("{:>16.3}  ", cos);
        }
        println!();
    }
}

// ── Diagnostic 2 ────────────────────────────────────────────────────────

fn diagnostic_2_top_activators(clfs: &IntentClassifiers, pack: &SeedPack) {
    println!("\n=== 2. Top-K activating phrases per classifier ===");
    println!("(Project every seed phrase through every classifier;");
    println!(" if classifier X's top phrases come from dim Y's pole, X is hijacking Y's signal)");

    let mut all_phrases: Vec<(String, &'static str, &'static str)> = Vec::new();
    for (dim_name, pair) in dims(pack) {
        for s in &pair.high_pole {
            all_phrases.push((s.clone(), dim_name, "high"));
        }
        for s in &pair.low_pole {
            all_phrases.push((s.clone(), dim_name, "low"));
        }
        for cf in &pair.pairs {
            all_phrases.push((cf.high.clone(), dim_name, "high"));
            all_phrases.push((cf.low.clone(), dim_name, "low"));
        }
    }

    println!(
        "\nFeaturizing all {} seed phrases (one-time, reuses model)...",
        all_phrases.len()
    );
    let featurized: Vec<(String, &str, &str, Vec<f32>)> = all_phrases
        .iter()
        .map(|(s, d, p)| (s.clone(), *d, *p, featurize(s)))
        .collect();

    let classifiers: [(&str, &Classifier); 4] = [
        ("conviction", &clfs.conviction),
        ("prediction_error", &clfs.prediction_error),
        ("arousal", &clfs.arousal),
        ("curiosity", &clfs.curiosity),
    ];

    const TOP_K: usize = 8;

    for (clf_name, clf) in &classifiers {
        println!(
            "\n--- {} classifier — top {} activating phrases ---",
            clf_name, TOP_K
        );

        let mut scored: Vec<(f32, &str, &str, &str)> = featurized
            .iter()
            .map(|(s, d, p, feats)| {
                let score = sigmoid(clf.bias + dot(feats, &clf.weights));
                (score, s.as_str(), *d, *p)
            })
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let mut hijack_count = 0;
        for (score, phrase, src_dim, src_pole) in scored.iter().take(TOP_K) {
            let want = format!("{clf_name}/high");
            let got = format!("{src_dim}/{src_pole}");
            let mark = if want == got { "✓" } else { "✗" };
            if want != got {
                hijack_count += 1;
            }
            println!(
                "  {} {:.3}  [{}]  \"{}\"",
                mark,
                score,
                got,
                truncate(phrase, 70)
            );
        }
        println!(
            "  → {}/{} from this dim's high pole; {}/{} hijacked from elsewhere",
            TOP_K - hijack_count,
            TOP_K,
            hijack_count,
            TOP_K
        );
    }
}

// ── Diagnostic 3 ────────────────────────────────────────────────────────

fn diagnostic_3_paraphrase_invariance(clfs: &IntentClassifiers) {
    println!("\n=== 3. Paraphrase invariance test ===");
    println!("(Each row scores the original sentence and 2 intent-preserving paraphrases.");
    println!(" High score deltas reveal classifiers keying on surface form rather than intent.)");

    let triplets: &[(&str, [&str; 3])] = &[
        (
            "conviction-high",
            [
                "I am absolutely certain that the meeting is at 3pm",
                "Without question the meeting is at 3pm",
                "I'm 100% sure the meeting is at 3pm",
            ],
        ),
        (
            "conviction-low",
            [
                "I think maybe the meeting was rescheduled",
                "Perhaps the meeting was rescheduled",
                "It might be that the meeting was rescheduled",
            ],
        ),
        (
            "prediction_error-high",
            [
                "Actually no, the meeting is at 3pm not 2pm",
                "Wait, scratch that — the meeting is at 3pm not 2pm",
                "Let me correct that — the meeting is at 3pm not 2pm",
            ],
        ),
        (
            "arousal-high",
            [
                "This is incredibly important — please respond now",
                "This is critical, please respond now",
                "This is urgent — please respond immediately",
            ],
        ),
        (
            "curiosity-high",
            [
                "What time was the meeting?",
                "Can you tell me what time the meeting was?",
                "Look up when the meeting was",
            ],
        ),
    ];

    let classifiers: [(&str, &Classifier); 4] = [
        ("conv", &clfs.conviction),
        ("pe", &clfs.prediction_error),
        ("aro", &clfs.arousal),
        ("cur", &clfs.curiosity),
    ];

    println!();
    print!("{:<28}  ", "label");
    for (n, _) in &classifiers {
        print!("{:>10}  ", n);
    }
    print!("{:>10}", "max-Δ");
    println!();
    println!("{}", "-".repeat(28 + 4 * 12 + 12));

    for (label, paras) in triplets {
        let mut all_scores: [Vec<f32>; 4] = [vec![], vec![], vec![], vec![]];

        for variant in paras {
            let feats = featurize(variant);
            for (i, (_, clf)) in classifiers.iter().enumerate() {
                let s = sigmoid(clf.bias + dot(&feats, &clf.weights));
                all_scores[i].push(s);
            }
        }

        let mut max_delta = 0.0f32;
        print!("{:<28}  ", label);
        for scores in &all_scores {
            let mean = scores.iter().sum::<f32>() / scores.len() as f32;
            let lo = scores.iter().cloned().fold(f32::INFINITY, f32::min);
            let hi = scores.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            let delta = hi - lo;
            if delta > max_delta {
                max_delta = delta;
            }
            print!("{:>5.2}±{:.2}  ", mean, delta / 2.0);
        }
        println!("{:>10.2}", max_delta);

        for variant in paras {
            println!("  · \"{}\"", truncate(variant, 70));
        }
        println!();
    }

    println!(
        "(Each cell: mean score ± half-range. max-Δ = largest score range across the 4 dims.)"
    );
    println!(
        "(A robust classifier has max-Δ ≪ 0.1; values > 0.2 mean significant surface-form sensitivity.)"
    );
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(n - 1).collect();
        out.push('…');
        out
    }
}
