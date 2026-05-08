//! Train per-dimension intent classifiers from `seed_pack.yaml`.
//!
//! Run: `cargo run --release --example gen_intent_classifiers`
//!
//! For each of the four intent dimensions, trains a binary logistic
//! regression classifier where:
//!   - positives = this dim's `high_pole` phrases + each `pairs[].high`
//!   - negatives = this dim's `low_pole` phrases + each `pairs[].low` PLUS
//!     every phrase from the other three dimensions (high_pole, low_pole,
//!     and both sides of each pair).
//!
//! Cross-class negatives push the learned direction toward what's *unique*
//! to this dim's high pole rather than the generic high-vs-low direction.
//! Pearl-flavored: this is adjusting for "first-person assertion shape"
//! as a confounder between conviction and prediction_error.
//!
//! On top of the standard logistic loss, we add a Bradley-Terry style
//! pairwise contrastive term over `pairs`. For each (h, l) pair in this
//! dim, we minimize `-log sigmoid(w·(h - l))`, which forces the score
//! gap between counterfactually-paired sentences (same topic, flipped
//! intent) to be positive. Pearl Level-3: controlled experiments where
//! topical content is held fixed and only the intent direction varies,
//! which strips topic out of the learned weights.
//!
//! Each phrase's feature vector is the BGE/MiniLM sentence embedding (384)
//! concatenated with hand-crafted lexical features (34) — total 418 dims.
//! Lexical features serve as a front-door mediator capturing intent-causing
//! syntax (modals, person, mood) cleanly, without the topic confounding
//! the embedding carries.
//!
//! Output: `src/intent_classifiers/<dim>.bin`, format = 418 f32 weights
//! followed by 1 f32 bias, all little-endian.

#[path = "shared/mod.rs"]
mod shared;

use legend::intent_classifiers::featurize;
use legend::math::sigmoid;
use shared::{dims, load_seed_pack};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

// Per-dim featurized pools. `pair_high[i]` and `pair_low[i]` are the two
// sides of the i-th counterfactual pair.
struct Featurized {
    high: Vec<Vec<f32>>,
    low: Vec<Vec<f32>>,
    pair_high: Vec<Vec<f32>>,
    pair_low: Vec<Vec<f32>>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pack = load_seed_pack(Path::new("seed_pack.yaml"))?;
    let dim_pairs = dims(&pack);

    println!("Embedding all phrases + extracting lexical features...");
    let mut features: Vec<Featurized> = Vec::with_capacity(4);
    for (_, pair) in &dim_pairs {
        let high = featurize_pool(&pair.high_pole);
        let low = featurize_pool(&pair.low_pole);
        let pair_high: Vec<Vec<f32>> = pair.pairs.iter().map(|p| featurize(&p.high)).collect();
        let pair_low: Vec<Vec<f32>> = pair.pairs.iter().map(|p| featurize(&p.low)).collect();
        features.push(Featurized {
            high,
            low,
            pair_high,
            pair_low,
        });
    }

    let out_dir = Path::new("src/intent_classifiers");
    std::fs::create_dir_all(out_dir)?;

    for (dim_idx, (dim_name, _)) in dim_pairs.iter().enumerate() {
        let mut x: Vec<&Vec<f32>> = Vec::new();
        let mut y: Vec<f32> = Vec::new();

        // This dim: high pool + pair.high → positives, low pool + pair.low → negatives.
        for emb in &features[dim_idx].high {
            x.push(emb);
            y.push(1.0);
        }
        for emb in &features[dim_idx].pair_high {
            x.push(emb);
            y.push(1.0);
        }
        for emb in &features[dim_idx].low {
            x.push(emb);
            y.push(0.0);
        }
        for emb in &features[dim_idx].pair_low {
            x.push(emb);
            y.push(0.0);
        }
        // Cross-class: every phrase from every other dim (pools and pair sides) is a negative.
        for (other_idx, other) in features.iter().enumerate() {
            if other_idx == dim_idx {
                continue;
            }
            for emb in &other.high {
                x.push(emb);
                y.push(0.0);
            }
            for emb in &other.low {
                x.push(emb);
                y.push(0.0);
            }
            for emb in &other.pair_high {
                x.push(emb);
                y.push(0.0);
            }
            for emb in &other.pair_low {
                x.push(emb);
                y.push(0.0);
            }
        }

        // Pair gradient list: only this dim's own pairs (cross-dim differencing
        // would mix topics and wash out the controlled-experiment signal).
        let pairs: Vec<(&Vec<f32>, &Vec<f32>)> = features[dim_idx]
            .pair_high
            .iter()
            .zip(features[dim_idx].pair_low.iter())
            .collect();

        let n_pos = y.iter().filter(|&&v| v > 0.5).count();
        let n_neg = y.len() - n_pos;
        // Class balancing: each positive contributes `n_neg/n_pos` times a
        // negative's gradient so the imbalanced negative set (~7:1) doesn't
        // drown out the positive signal.
        let pos_weight = n_neg as f32 / n_pos as f32;

        let (weights, bias) = train(
            &x, &y, &pairs, pos_weight, /* pair_weight = */ 1.0,
            /* lambda      = */ 0.01, /* lr          = */ 0.5,
            /* epochs      = */ 5000,
        );

        let file_path = out_dir.join(format!("{dim_name}.bin"));
        let mut writer = BufWriter::new(File::create(&file_path)?);
        for &w in &weights {
            writer.write_all(&w.to_le_bytes())?;
        }
        writer.write_all(&bias.to_le_bytes())?;
        writer.flush()?;

        let train_loss = log_loss(&x, &y, &weights, bias, pos_weight);
        let pair_loss = pair_loss(&pairs, &weights);
        println!(
            "wrote {} (pos={}, neg={}, pairs={}, pos_weight={:.2}, log_loss={:.4}, pair_loss={:.4})",
            file_path.display(),
            n_pos,
            n_neg,
            pairs.len(),
            pos_weight,
            train_loss,
            pair_loss
        );
    }

    Ok(())
}

fn featurize_pool(phrases: &[String]) -> Vec<Vec<f32>> {
    phrases.iter().map(|s| featurize(s)).collect()
}

// Logistic regression with class-weighted gradient + L2, plus a Bradley-Terry
// pairwise contrastive term. Plain full-batch gradient descent — small n,
// simple and stable.
//
// Gradients per epoch:
//   - Standard:    g_log[j]  = mean over samples of  (sigmoid(w·x + b) - y) * class_w * x[j]
//   - Contrastive: g_pair[j] = mean over pairs of    (sigmoid(d) - 1) * (h[j] - l[j])
//                  where d = w·(h - l). Bias cancels in the difference.
//   - L2:          g_l2[j]   = lambda * w[j]
//   - Combined:    w[j] -= lr * (g_log[j] + pair_weight * g_pair[j] + g_l2[j])
#[allow(clippy::too_many_arguments)]
fn train(
    x: &[&Vec<f32>],
    y: &[f32],
    pairs: &[(&Vec<f32>, &Vec<f32>)],
    pos_weight: f32,
    pair_weight: f32,
    lambda: f32,
    lr: f32,
    epochs: usize,
) -> (Vec<f32>, f32) {
    let n = x.len();
    let dim = x[0].len();
    let mut weights = vec![0.0f32; dim];
    let mut bias = 0.0f32;

    for _ in 0..epochs {
        let mut grad_w = vec![0.0f32; dim];
        let mut grad_b = 0.0f32;
        let mut total_weight = 0.0f32;

        for i in 0..n {
            let mut logit = bias;
            for j in 0..dim {
                logit += weights[j] * x[i][j];
            }
            let prob = sigmoid(logit);
            let class_w = if y[i] > 0.5 { pos_weight } else { 1.0 };
            let err = (prob - y[i]) * class_w;

            for j in 0..dim {
                grad_w[j] += err * x[i][j];
            }
            grad_b += err;
            total_weight += class_w;
        }

        let scale = 1.0 / total_weight;
        let mut combined = vec![0.0f32; dim];
        for j in 0..dim {
            combined[j] = grad_w[j] * scale;
        }

        if !pairs.is_empty() {
            let pair_scale = pair_weight / pairs.len() as f32;
            for (h, l) in pairs {
                let mut d = 0.0f32;
                for j in 0..dim {
                    d += weights[j] * (h[j] - l[j]);
                }
                let factor = sigmoid(d) - 1.0;
                for j in 0..dim {
                    combined[j] += pair_scale * factor * (h[j] - l[j]);
                }
            }
        }

        for j in 0..dim {
            weights[j] -= lr * (combined[j] + lambda * weights[j]);
        }
        bias -= lr * grad_b * scale;
    }

    (weights, bias)
}

fn log_loss(x: &[&Vec<f32>], y: &[f32], weights: &[f32], bias: f32, pos_weight: f32) -> f32 {
    let mut total = 0.0f32;
    let mut total_w = 0.0f32;
    for i in 0..x.len() {
        let mut logit = bias;
        for j in 0..weights.len() {
            logit += weights[j] * x[i][j];
        }
        let prob = sigmoid(logit).clamp(1e-9, 1.0 - 1e-9);
        let class_w = if y[i] > 0.5 { pos_weight } else { 1.0 };
        let loss = -(y[i] * prob.ln() + (1.0 - y[i]) * (1.0 - prob).ln());
        total += loss * class_w;
        total_w += class_w;
    }
    total / total_w
}

fn pair_loss(pairs: &[(&Vec<f32>, &Vec<f32>)], weights: &[f32]) -> f32 {
    if pairs.is_empty() {
        return 0.0;
    }
    let mut total = 0.0f32;
    for (h, l) in pairs {
        let mut d = 0.0f32;
        for j in 0..weights.len() {
            d += weights[j] * (h[j] - l[j]);
        }
        let p = sigmoid(d).clamp(1e-9, 1.0 - 1e-9);
        total += -p.ln();
    }
    total / pairs.len() as f32
}
