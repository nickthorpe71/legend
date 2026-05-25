//! Score an input against every individual prototype in a named region,
//! sorted by cosine. Diagnoses cases where max-pool over a region is
//! pulled high by a single surface-overlap prototype.
//!
//! Run: `cargo run --release --example score_per_prototype -- <region_name> "phrase"`
//!
//! Throwaway diagnostic — toss before prod.

use legend::embed::embed_text;
use legend::math::dot;
use legend::seed::load_seed_graph;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: cargo run --example score_per_prototype -- <region_name> \"phrase\"");
        std::process::exit(1);
    }
    let region_name = &args[1];
    let input = &args[2];

    let hypergraph = load_seed_graph();
    let region_id = hypergraph
        .by_name
        .get(region_name)
        .and_then(|ids| ids.first().copied())
        .unwrap_or_else(|| {
            eprintln!("region {region_name:?} not found");
            std::process::exit(1);
        });
    let protos = hypergraph
        .region_prototypes
        .get(&region_id)
        .cloned()
        .unwrap_or_default();

    let embedding = embed_text(input);
    let mut scored: Vec<(String, f32)> = protos
        .iter()
        .map(|&p| {
            let elem = &hypergraph.elements[p.0 as usize];
            // Look up the example text by recovering from the proto's
            // name — but it's just "<region>_proto_<i>"; the example
            // text lives only in the YAML. For diagnostic value, show
            // cosine + the proto id; for the actual text, the user
            // looks it up in seed_pack.yaml by index.
            let label = elem.names.first().cloned().unwrap_or_default();
            let sim = dot(&embedding, &elem.embedding);
            (label, sim)
        })
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    println!("input:  {input}");
    println!("region: {region_name} ({} prototypes)", scored.len());
    println!();
    println!("{:<30} {:>8}", "prototype", "cosine");
    println!("{:-<30} {:->8}", "", "");
    for (label, sim) in &scored {
        println!("{label:<30} {sim:>+8.4}");
    }
}
