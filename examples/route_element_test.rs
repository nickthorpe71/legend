//! Experimental: route a single-element embedding against each region's
//! own `.embedding` (no prototypes, no Mahalanobis). Print top-3 regions
//! per test input so we can see whether single-embedding-per-region
//! holds up before deleting the prototype machinery.
//!
//! Run: `cargo run --release --example route_element_test`

use legend::embed::embed_text;
use legend::math::dot;
use legend::seed::load_seed_graph;
use legend::types::{ElementId, Hypergraph};

fn main() {
    let hg = load_seed_graph();

    // Test inputs and the region we'd intuitively expect each to land in.
    // `None` = we're curious where it goes but have no strong prior.
    let cases: &[(&str, Option<&str>)] = &[
        ("Brantford", Some("locations")),
        ("Berlin", Some("locations")),
        ("Paris", Some("locations")),
        ("Times Square", Some("locations")),
        ("Nick", Some("entities")),
        ("Sarah", Some("entities")),
        ("Dr. Rao", Some("entities")),
        ("Apollo project", Some("entities")),
        ("Tuesday", Some("time")),
        ("3pm", Some("time")),
        ("yesterday", Some("time")),
        ("3 years", Some("quantities")),
        ("6 pounds", Some("quantities")),
        ("$42", Some("quantities")),
        ("dentist", Some("entities")),
        ("happy", None),
        ("the meeting", Some("events")),
        ("she changed her mind", Some("change_history")),
        ("I prefer tea over coffee", Some("preferences")),
    ];

    // Children of GENESIS = signal regions we route against. Skip VOID
    // and its subtree — closed-class regions aren't in this experiment.
    let region_ids: Vec<ElementId> = hg
        .region_children
        .get(&hg.genesis)
        .cloned()
        .unwrap_or_default();

    println!(
        "{} signal regions under GENESIS\n",
        region_ids.len()
    );

    let mut correct_top1 = 0usize;
    let mut total_with_expected = 0usize;

    for (input, expected) in cases {
        let emb = embed_text(input);
        let scored = score_regions(&emb, &region_ids, &hg);

        let top1 = scored
            .first()
            .map(|(name, _)| name.as_str())
            .unwrap_or("?");
        let mark = match expected {
            Some(want) => {
                total_with_expected += 1;
                if top1 == *want {
                    correct_top1 += 1;
                    "✓"
                } else {
                    "✗"
                }
            }
            None => " ",
        };

        let expected_str = expected.unwrap_or("(no prior)");
        print!("{mark} {input:<28}  expected={expected_str:<14}  top3:");
        for (name, score) in scored.iter().take(3) {
            print!("  {name}({score:+.3})");
        }
        // Where did the expected region rank?
        if let Some(want) = expected {
            let rank = scored
                .iter()
                .position(|(n, _)| n == want)
                .map(|i| i + 1);
            match rank {
                Some(r) if r > 3 => print!("  [expected at rank {r}]"),
                None => print!("  [expected not in scores]"),
                _ => {}
            }
        }
        println!();
    }

    if total_with_expected > 0 {
        println!(
            "\ntop-1 accuracy on cases with priors: {correct_top1}/{total_with_expected} ({:.0}%)",
            100.0 * correct_top1 as f32 / total_with_expected as f32,
        );
    }
}

fn score_regions(
    input_emb: &[f32],
    region_ids: &[ElementId],
    hg: &Hypergraph,
) -> Vec<(String, f32)> {
    let mut out: Vec<(String, f32)> = region_ids
        .iter()
        .map(|&id| {
            let region = &hg.elements[id.0 as usize];
            let name = region.names.first().cloned().unwrap_or_default();
            let score = dot(input_emb, &region.embedding);
            (name, score)
        })
        .collect();
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    out
}
