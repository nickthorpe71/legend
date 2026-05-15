//! Phase-3 validation: route the v3 18-case fixture against the
//! *production* seed graph (not a hand-built anchor set) and report
//! top-1 accuracy. Compares the regenerated pack's contextualized
//! prototype embeddings against a synthesized bare-name baseline.
//!
//! Each region's anchor strategy:
//!   - **proto-max** — for each region under GENESIS, take the max
//!     cosine across that region's 20 prototype embeddings. This
//!     matches `route_regions`'s actual scoring logic.
//!   - **bare-name** — `embed_text(region.names[0])`, the pre-phase-2
//!     baseline. Synthesized on the fly so we don't need a copy of
//!     the old pack.
//!
//! Run: `cargo run --release --example route_seed_pack_test`

use legend::embed::{embed_span_in_context, embed_text};
use legend::math::dot;
use legend::seed::load_seed_graph;
use legend::types::{ElementId, Hypergraph};

/// Hand-picked test fixture mirroring `route_element_test_v3.rs`.
/// `expected = None` means "no strong prior" — counted in the run
/// totals but not in the accuracy ratio.
const CASES: &[(&str, &str, Option<&str>)] = &[
    (
        "Nick lived in Brantford for 3 years.",
        "Brantford",
        Some("locations"),
    ),
    (
        "She moved to Berlin last year.",
        "Berlin",
        Some("locations"),
    ),
    ("He flew to Paris on Tuesday.", "Paris", Some("locations")),
    ("We met at Times Square.", "Times Square", Some("locations")),
    (
        "Nick lived in Brantford for 3 years.",
        "Nick",
        Some("entities"),
    ),
    ("Sarah called me yesterday.", "Sarah", Some("entities")),
    (
        "Dr. Rao changed the appointment.",
        "Dr. Rao",
        Some("entities"),
    ),
    (
        "The Apollo project shipped on time.",
        "Apollo project",
        Some("entities"),
    ),
    ("The meeting is on Tuesday.", "Tuesday", Some("time")),
    ("Her flight lands at 3pm.", "3pm", Some("time")),
    ("He called yesterday afternoon.", "yesterday", Some("time")),
    (
        "Nick lived in Brantford for 3 years.",
        "3 years",
        Some("quantities"),
    ),
    (
        "The baby weighed 6 pounds at birth.",
        "6 pounds",
        Some("quantities"),
    ),
    ("The book cost $42.", "$42", Some("quantities")),
    (
        "The dentist saw three patients.",
        "dentist",
        Some("entities"),
    ),
    ("Sarah is happy today.", "happy", None),
    ("The meeting is scheduled.", "meeting", Some("events")),
    (
        "She changed her mind about it.",
        "changed her mind",
        Some("change_history"),
    ),
    (
        "I prefer tea over coffee.",
        "prefer tea",
        Some("preferences"),
    ),
];

fn main() {
    let hg = load_seed_graph();
    let genesis_children: Vec<ElementId> = hg
        .region_children
        .get(&hg.genesis)
        .cloned()
        .unwrap_or_default();

    println!(
        "{} signal regions under GENESIS, {} test cases\n",
        genesis_children.len(),
        CASES.len(),
    );

    // (query × anchor) matrix. 2 query strategies × 4 anchor strategies.
    let mut acc = [[0usize; 4]; 2];
    let mut total_with_expected = 0usize;

    for (sentence, span, expected) in CASES {
        let span_start = sentence
            .find(span)
            .unwrap_or_else(|| panic!("span {span:?} not in {sentence:?}"));
        let q_span = embed_span_in_context(sentence, span_start, span_start + span.len())
            .expect("query span must resolve");
        let q_sentence =
            embed_span_in_context(sentence, 0, sentence.len()).expect("sentence span must resolve");

        // Anchor columns: bare-name, K=1 (max), K=3 (production), K=∞ (full mean).
        let preds = [
            [
                pick_by_bare_name(&q_span, &genesis_children, &hg).0,
                pick_by_mean_top_k(&q_span, &genesis_children, &hg, 1).0,
                pick_by_mean_top_k(&q_span, &genesis_children, &hg, 3).0,
                pick_by_mean_top_k(&q_span, &genesis_children, &hg, usize::MAX).0,
            ],
            [
                pick_by_bare_name(&q_sentence, &genesis_children, &hg).0,
                pick_by_mean_top_k(&q_sentence, &genesis_children, &hg, 1).0,
                pick_by_mean_top_k(&q_sentence, &genesis_children, &hg, 3).0,
                pick_by_mean_top_k(&q_sentence, &genesis_children, &hg, usize::MAX).0,
            ],
        ];

        if let Some(e) = expected {
            total_with_expected += 1;
            for q in 0..2 {
                for a in 0..4 {
                    if &name_of(&hg, preds[q][a]) == e {
                        acc[q][a] += 1;
                    }
                }
            }
        }
    }

    println!();
    println!("  {:─<92}", "");
    println!(
        "  {:<24} | {:<13} | {:<13} | {:<13} | {:<13}",
        "query ↓ / anchor →", "bare-name", "K=1 (max)", "K=3 (prod)", "K=∞ (mean)"
    );
    println!("  {:─<92}", "");
    let pct = |k: usize| 100.0 * k as f64 / total_with_expected as f64;
    let row = |label: &str, r: &[usize]| {
        println!(
            "  {label:<24} | {:>3}/{:<2} ({:>3.0}%) | {:>3}/{:<2} ({:>3.0}%) | {:>3}/{:<2} ({:>3.0}%) | {:>3}/{:<2} ({:>3.0}%)",
            r[0],
            total_with_expected,
            pct(r[0]),
            r[1],
            total_with_expected,
            pct(r[1]),
            r[2],
            total_with_expected,
            pct(r[2]),
            r[3],
            total_with_expected,
            pct(r[3]),
        );
    };
    row("span-in-context", &acc[0]);
    row("full sentence", &acc[1]);
    println!("  {:─<92}", "");
}

/// Score each region by cosine against `embed_text(region.names[0])`.
/// Returns (best_region_id, score).
fn pick_by_bare_name(query: &[f32], regions: &[ElementId], hg: &Hypergraph) -> (ElementId, f32) {
    let mut best: (ElementId, f32) = (regions[0], f32::NEG_INFINITY);
    for &id in regions {
        let name = &hg.elements[id.0 as usize].names[0];
        let anchor = embed_text(name);
        let s = dot(query, &anchor);
        if s > best.1 {
            best = (id, s);
        }
    }
    best
}

/// Score each region by mean cosine over its top-K prototypes.
/// K=1 ≡ max-pool, K=∞ ≡ full mean. Production `route_regions`
/// uses K = `policy.cosine_top_k` (default 3).
fn pick_by_mean_top_k(
    query: &[f32],
    regions: &[ElementId],
    hg: &Hypergraph,
    k: usize,
) -> (ElementId, f32) {
    let mut best: (ElementId, f32) = (regions[0], f32::NEG_INFINITY);
    for &id in regions {
        let Some(protos) = hg.region_prototypes.get(&id) else {
            continue;
        };
        let mut sims: Vec<f32> = protos
            .iter()
            .map(|p_id| dot(query, &hg.elements[p_id.0 as usize].embedding))
            .collect();
        sims.sort_by(|a, b| b.partial_cmp(a).unwrap());
        let kk = k.min(sims.len()).max(1);
        let mean = sims.iter().take(kk).sum::<f32>() / kk as f32;
        if mean > best.1 {
            best = (id, mean);
        }
    }
    best
}

fn name_of(hg: &Hypergraph, id: ElementId) -> String {
    hg.elements[id.0 as usize]
        .names
        .first()
        .cloned()
        .unwrap_or_else(|| format!("?{}?", id.0))
}
