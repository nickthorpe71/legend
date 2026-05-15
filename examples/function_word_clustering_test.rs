//! Experimental — does contextualized token clustering put function
//! words near each other, separate from content words?
//!
//! Question: if we drop `Polarity` and let the DAG self-organize by
//! embedding similarity alone, will `the` / `of` / `and` / `is` /
//! `in` / `for` / `to` reliably cluster into one branch (so downstream
//! frequency-based filtering can deprioritize them as a unit)?
//!
//! Test:
//! 1. Embed every token in 10 sample sentences using contextualized
//!    BERT outputs (the per-token vectors `embed_sequence_with_offsets`
//!    returns). Special tokens [CLS] / [SEP] skipped.
//! 2. For each token instance, find its top-5 nearest neighbors among
//!    *all* token instances across all sentences (by cosine).
//! 3. Inspect: do function-word instances mostly find other
//!    function-word instances? Do content-word instances mostly find
//!    other content-word instances?
//! 4. Aggregate statistic: average cosine within the function-word
//!    class vs. between function and content. Higher within-class than
//!    across-class = the hypothesis holds; routing alone would cluster
//!    function words.
//!
//! Run: `cargo run --release --example function_word_clustering_test`

use legend::embed::{EMBEDDING_DIM, embed_sequence_with_offsets};
use legend::math::dot;

/// Known function words to test. Lower-cased; matched case-insensitively
/// to surface tokens.
const FUNCTION_WORDS: &[&str] = &[
    "the", "a", "an", "of", "in", "on", "at", "to", "for", "and", "or", "but",
    "is", "was", "are", "were", "be", "been",
    "i", "he", "she", "we", "they", "me", "him", "her", "us", "them",
    "this", "that", "these", "those",
    "with", "by", "from", "over", "into", "out",
];

fn is_function_word(token: &str) -> bool {
    let lower = token.to_lowercase();
    FUNCTION_WORDS.iter().any(|fw| *fw == lower.as_str())
}

struct TokenInstance {
    sentence_idx: usize,
    token_text: String,
    embedding: Vec<f32>, // L2-normalized
}

fn main() {
    let sentences = [
        "Nick lived in Brantford for 3 years.",
        "Sarah called me yesterday afternoon.",
        "The dentist scheduled the appointment for Friday.",
        "We met at Times Square on Tuesday.",
        "The book cost $42 at the store.",
        "Maya leads the design team.",
        "I prefer tea over coffee.",
        "The deploy went live this morning.",
        "She moved to Berlin last year.",
        "John finished the report on time.",
    ];

    // Collect every contextualized token across all sentences.
    let mut instances: Vec<TokenInstance> = Vec::new();
    for (si, sentence) in sentences.iter().enumerate() {
        let (sequence, offsets) = embed_sequence_with_offsets(sentence);
        for (t, &(start, end)) in offsets.iter().enumerate() {
            if start == 0 && end == 0 {
                continue; // [CLS] / [SEP] / padding
            }
            let token_text = sentence[start..end].to_string();
            // Skip standalone punctuation tokens. We want a clean
            // function-vs-content signal; punctuation muddies it.
            if !token_text.chars().any(|c| c.is_alphanumeric()) {
                continue;
            }
            let base = t * EMBEDDING_DIM;
            let raw = &sequence[base..base + EMBEDDING_DIM];
            let mut v = raw.to_vec();
            let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
            for x in &mut v {
                *x /= norm;
            }
            instances.push(TokenInstance {
                sentence_idx: si,
                token_text,
                embedding: v,
            });
        }
    }

    println!("collected {} token instances across {} sentences\n", instances.len(), sentences.len());

    // ── 1. Top-5 neighbors for selected probe tokens ────────────────
    let probes_fn: &[&str] = &["the", "in", "for", "of", "to", "on"];
    let probes_content: &[&str] = &["Nick", "Brantford", "Sarah", "dentist", "Berlin", "John"];

    println!("FUNCTION-WORD PROBES (expect neighbors to be other function words):");
    for probe in probes_fn {
        show_neighbors(probe, &instances, 6);
    }
    println!("\nCONTENT-WORD PROBES (expect neighbors to be other content words):");
    for probe in probes_content {
        show_neighbors(probe, &instances, 6);
    }

    // ── 2. Class-level cosine statistics ────────────────────────────
    println!("\nCLASS-LEVEL AVERAGE COSINES (across-pair, not self):");
    let mut fn_fn_sum = 0.0f32;
    let mut fn_fn_count = 0usize;
    let mut fn_ct_sum = 0.0f32;
    let mut fn_ct_count = 0usize;
    let mut ct_ct_sum = 0.0f32;
    let mut ct_ct_count = 0usize;

    for i in 0..instances.len() {
        for j in (i + 1)..instances.len() {
            let cos = dot(&instances[i].embedding, &instances[j].embedding);
            let a_fn = is_function_word(&instances[i].token_text);
            let b_fn = is_function_word(&instances[j].token_text);
            match (a_fn, b_fn) {
                (true, true) => { fn_fn_sum += cos; fn_fn_count += 1; }
                (false, false) => { ct_ct_sum += cos; ct_ct_count += 1; }
                _ => { fn_ct_sum += cos; fn_ct_count += 1; }
            }
        }
    }
    println!(
        "  fn ↔ fn:   avg cos = {:.4}  ({} pairs)",
        fn_fn_sum / fn_fn_count.max(1) as f32,
        fn_fn_count,
    );
    println!(
        "  fn ↔ ct:   avg cos = {:.4}  ({} pairs)",
        fn_ct_sum / fn_ct_count.max(1) as f32,
        fn_ct_count,
    );
    println!(
        "  ct ↔ ct:   avg cos = {:.4}  ({} pairs)",
        ct_ct_sum / ct_ct_count.max(1) as f32,
        ct_ct_count,
    );

    // Interpretation:
    // - If fn↔fn > fn↔ct AND ct↔ct > fn↔ct: classes are separable.
    // - If they're all similar: contextualization doesn't separate
    //   function words from content words.

    // ── 3. Per-function-word centroid distance to "function-word
    //     centroid" vs "content-word centroid" ─────────────────────
    println!("\nCENTROID TEST:");
    let (fn_centroid, ct_centroid) = build_class_centroids(&instances);
    for probe in probes_fn {
        let centroid = build_token_centroid(probe, &instances);
        if let Some(c) = centroid {
            println!(
                "  '{probe}' → fn_centroid cos={:.3}, ct_centroid cos={:.3}, diff={:+.3}",
                dot(&c, &fn_centroid),
                dot(&c, &ct_centroid),
                dot(&c, &fn_centroid) - dot(&c, &ct_centroid),
            );
        }
    }
    for probe in probes_content {
        let centroid = build_token_centroid(probe, &instances);
        if let Some(c) = centroid {
            println!(
                "  '{probe}' → fn_centroid cos={:.3}, ct_centroid cos={:.3}, diff={:+.3}",
                dot(&c, &fn_centroid),
                dot(&c, &ct_centroid),
                dot(&c, &fn_centroid) - dot(&c, &ct_centroid),
            );
        }
    }
}

fn show_neighbors(probe: &str, instances: &[TokenInstance], k: usize) {
    let probe_lc = probe.to_lowercase();
    // Pick the first instance matching the probe (case-insensitive).
    let probe_inst = match instances.iter().find(|i| i.token_text.to_lowercase() == probe_lc) {
        Some(p) => p,
        None => {
            println!("  '{probe}': not found in any sentence");
            return;
        }
    };
    let mut neighbors: Vec<(usize, f32)> = instances
        .iter()
        .enumerate()
        .filter(|(_, inst)| !std::ptr::eq(*inst, probe_inst))
        .map(|(i, inst)| (i, dot(&probe_inst.embedding, &inst.embedding)))
        .collect();
    neighbors.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    print!("  '{probe}' (s{} ctx) → top-{k}:", probe_inst.sentence_idx);
    for (idx, cos) in neighbors.iter().take(k) {
        let inst = &instances[*idx];
        let class = if is_function_word(&inst.token_text) { "·" } else { "★" };
        print!("  {class}{}({:.3})", inst.token_text, cos);
    }
    println!();
}

fn build_token_centroid(token: &str, instances: &[TokenInstance]) -> Option<Vec<f32>> {
    let lc = token.to_lowercase();
    let mut acc = vec![0.0f32; EMBEDDING_DIM];
    let mut n = 0usize;
    for inst in instances {
        if inst.token_text.to_lowercase() == lc {
            for i in 0..EMBEDDING_DIM {
                acc[i] += inst.embedding[i];
            }
            n += 1;
        }
    }
    if n == 0 {
        return None;
    }
    let inv = 1.0 / n as f32;
    for x in &mut acc {
        *x *= inv;
    }
    let norm: f32 = acc.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-12);
    for x in &mut acc {
        *x /= norm;
    }
    Some(acc)
}

fn build_class_centroids(instances: &[TokenInstance]) -> (Vec<f32>, Vec<f32>) {
    let mut fn_acc = vec![0.0f32; EMBEDDING_DIM];
    let mut fn_n = 0usize;
    let mut ct_acc = vec![0.0f32; EMBEDDING_DIM];
    let mut ct_n = 0usize;
    for inst in instances {
        let target = if is_function_word(&inst.token_text) {
            fn_n += 1;
            &mut fn_acc
        } else {
            ct_n += 1;
            &mut ct_acc
        };
        for i in 0..EMBEDDING_DIM {
            target[i] += inst.embedding[i];
        }
    }
    for x in &mut fn_acc {
        *x /= fn_n.max(1) as f32;
    }
    for x in &mut ct_acc {
        *x /= ct_n.max(1) as f32;
    }
    let fn_norm: f32 = fn_acc.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-12);
    for x in &mut fn_acc {
        *x /= fn_norm;
    }
    let ct_norm: f32 = ct_acc.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-12);
    for x in &mut ct_acc {
        *x /= ct_norm;
    }
    (fn_acc, ct_acc)
}
