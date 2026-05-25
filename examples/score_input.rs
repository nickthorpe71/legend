//! Score an input phrase against every GENESIS-child region's
//! prototypes. Throwaway diagnostic for calibrating `route_regions` thresholds.
//!
//! Run: `cargo run --release --example score_input -- "your phrase"`
//!
//! Output: each of GENESIS's region children, sorted by best cosine
//! against the input embedding. Lets you see what `descend_threshold`,
//! `leaf_vigilance`, and `region_activation_threshold` would actually
//! gate at, instead of guessing from "did it activate?".

use legend::embed::embed_text;
use legend::math::dot;
use legend::seed::load_seed_graph;
use legend::types::ElementId;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: cargo run --example score_input -- \"phrase\"");
        std::process::exit(1);
    }
    let input = &args[1];
    let hypergraph = load_seed_graph();
    let embedding = embed_text(input);

    let children = hypergraph
        .region_children
        .get(&hypergraph.genesis)
        .cloned()
        .unwrap_or_default();

    let mut scored: Vec<(String, f32, ElementId)> = Vec::with_capacity(children.len());
    for child in children {
        let Some(protos) = hypergraph.region_prototypes.get(&child) else {
            continue;
        };
        let mut best: Option<(ElementId, f32)> = None;
        for &proto in protos {
            let sim = dot(&embedding, &hypergraph.elements[proto.0 as usize].embedding);
            if best.is_none_or(|(_, s)| sim > s) {
                best = Some((proto, sim));
            }
        }
        if let Some((p, s)) = best {
            let name = hypergraph.elements[child.0 as usize]
                .names
                .first()
                .cloned()
                .unwrap_or_else(|| format!("?{}?", child.0));
            scored.push((name, s, p));
        }
    }
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    println!("input: {input}");
    println!();
    println!("{:<20} {:>7}  best prototype id", "region", "cosine");
    println!("{:-<20} {:->7}  -----------------", "", "");
    for (name, score, proto) in scored {
        println!("{name:<20} {score:>+7.4}  {}", proto.0);
    }
}
