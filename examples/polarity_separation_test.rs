//! Phase-5 question: do contextualized embeddings separate
//! Polarity::Void and Polarity::Signal elements cleanly enough that
//! cosine routing alone could replace the `Polarity` flag + the
//! `void_filter` token-classification pass?
//!
//! Measures:
//!   1. Class centroids — mean of each polarity's normalized
//!      embeddings.
//!   2. Cosine of every element to BOTH centroids; classify by
//!      whichever is higher.
//!   3. Per-class classification accuracy + confusion details.
//!
//! Reads the regenerated seed graph (post-phase-2 contextualized
//! embeddings). No model calls.
//!
//! Run: `cargo run --release --example polarity_separation_test`

use legend::math::dot;
use legend::seed::load_seed_graph;
use legend::types::{Hypergraph, Polarity};

fn main() {
    let hg = load_seed_graph();

    // Gather embeddings by polarity. Skip elements with zero
    // embeddings (anchors VOID / GENESIS might have degenerate
    // vectors; check by norm) and class sentinels.
    let mut void_embs: Vec<&Vec<f32>> = Vec::new();
    let mut signal_embs: Vec<&Vec<f32>> = Vec::new();
    for e in &hg.elements {
        // Skip the class sentinels and the bare anchors; they have
        // hand-set degenerate embeddings.
        if e.id == hg.void || e.id == hg.genesis {
            continue;
        }
        match e.polarity {
            Polarity::Void => void_embs.push(&e.embedding),
            Polarity::Signal => signal_embs.push(&e.embedding),
        }
    }
    println!(
        "Polarity::Void   elements: {}\nPolarity::Signal elements: {}",
        void_embs.len(),
        signal_embs.len(),
    );

    let dim = void_embs[0].len();

    // Centroids.
    let void_centroid = centroid(&void_embs, dim);
    let signal_centroid = centroid(&signal_embs, dim);
    let centroid_cos = dot(&void_centroid, &signal_centroid);
    println!(
        "\nCentroid cosine (void vs signal): {centroid_cos:+.4}  \
         (1.0 = identical direction, 0.0 = orthogonal)",
    );

    // Within-class spread: mean cosine of each element to its own centroid.
    let void_self = mean_cos_to(&void_embs, &void_centroid);
    let signal_self = mean_cos_to(&signal_embs, &signal_centroid);
    println!(
        "\nMean cosine to own-class centroid:\n  void→void:     {void_self:+.4}\n  signal→signal: {signal_self:+.4}",
    );

    // Cross-class: mean cosine to opposite centroid.
    let void_cross = mean_cos_to(&void_embs, &signal_centroid);
    let signal_cross = mean_cos_to(&signal_embs, &void_centroid);
    println!(
        "Mean cosine to opposite-class centroid:\n  void→signal:   {void_cross:+.4}\n  signal→void:   {signal_cross:+.4}",
    );

    // Margin: difference between own-class similarity and cross-class.
    let void_margin = void_self - void_cross;
    let signal_margin = signal_self - signal_cross;
    println!(
        "\nClass margin (own-centroid cos − opposite-centroid cos):\n  void margin:    {void_margin:+.4}\n  signal margin:  {signal_margin:+.4}",
    );

    // Per-element classification: which centroid is closer?
    let mut void_classified_void = 0usize;
    let mut signal_classified_signal = 0usize;
    let mut void_confused: Vec<String> = Vec::new();
    let mut signal_confused: Vec<String> = Vec::new();
    for e in &hg.elements {
        if e.id == hg.void || e.id == hg.genesis {
            continue;
        }
        let cv = dot(&e.embedding, &void_centroid);
        let cs = dot(&e.embedding, &signal_centroid);
        let predicted_void = cv > cs;
        let actual_void = matches!(e.polarity, Polarity::Void);
        if actual_void && predicted_void {
            void_classified_void += 1;
        } else if !actual_void && !predicted_void {
            signal_classified_signal += 1;
        } else if actual_void {
            void_confused.push(format!(
                "{} (cv={cv:+.3}, cs={cs:+.3}, Δ={:+.3})",
                e.names[0],
                cs - cv
            ));
        } else {
            signal_confused.push(format!(
                "{} (cv={cv:+.3}, cs={cs:+.3}, Δ={:+.3})",
                e.names[0],
                cv - cs
            ));
        }
    }

    let void_acc = void_classified_void as f64 / void_embs.len() as f64;
    let signal_acc = signal_classified_signal as f64 / signal_embs.len() as f64;
    println!(
        "\nNearest-centroid classification accuracy:\n  void:   {}/{} ({:.0}%)\n  signal: {}/{} ({:.0}%)",
        void_classified_void,
        void_embs.len(),
        void_acc * 100.0,
        signal_classified_signal,
        signal_embs.len(),
        signal_acc * 100.0,
    );

    if !void_confused.is_empty() {
        println!(
            "\n{} void elements misclassified as signal:",
            void_confused.len()
        );
        for s in void_confused.iter().take(20) {
            println!("  {s}");
        }
        if void_confused.len() > 20 {
            println!("  ... and {} more", void_confused.len() - 20);
        }
    }
    if !signal_confused.is_empty() {
        println!(
            "\n{} signal elements misclassified as void:",
            signal_confused.len()
        );
        for s in signal_confused.iter().take(20) {
            println!("  {s}");
        }
        if signal_confused.len() > 20 {
            println!("  ... and {} more", signal_confused.len() - 20);
        }
    }

    println!("\n{:─<60}", "");
    println!("INTERPRETATION");
    println!("{:─<60}", "");
    let overall = (void_classified_void + signal_classified_signal) as f64
        / (void_embs.len() + signal_embs.len()) as f64;
    println!("Overall nearest-centroid classification: {:.0}%", overall * 100.0);
    if overall >= 0.95 {
        println!("→ Polarity is recoverable from cosine alone. Polarity field");
        println!("  + void_filter could be deprecated; routing would do the work.");
    } else if overall >= 0.80 {
        println!("→ Cosine separates the classes but not cleanly. Keep Polarity");
        println!("  as a backup filter; the misclassified elements would leak");
        println!("  through pure-cosine routing.");
    } else {
        println!("→ Cosine does NOT separate the classes well. Polarity is");
        println!("  doing real work — keep it.");
    }
}

fn centroid(embs: &[&Vec<f32>], dim: usize) -> Vec<f32> {
    let mut acc = vec![0.0f32; dim];
    for v in embs {
        for i in 0..dim {
            acc[i] += v[i];
        }
    }
    let n = embs.len() as f32;
    for x in &mut acc {
        *x /= n;
    }
    let norm: f32 = acc.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
    for x in &mut acc {
        *x /= norm;
    }
    acc
}

fn mean_cos_to(embs: &[&Vec<f32>], centroid: &[f32]) -> f32 {
    let mut sum = 0.0f32;
    for v in embs {
        sum += dot(v, centroid);
    }
    sum / embs.len() as f32
}

// Hypergraph and Polarity used only for type signatures of the
// public traits the loader returns; not directly invoked here.
#[allow(dead_code)]
fn _force_hg(_: &Hypergraph) {}
