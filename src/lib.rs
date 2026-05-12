pub mod embed;
pub mod inference;
pub mod intent_classifiers;
pub mod lexical_features;
pub mod math;
pub mod render;
pub mod seed;
pub mod steps;
pub mod types;

use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::SystemTime;

use seed::load_seed_graph;
use steps::adjust_policy::adjust_policy;
use steps::detect_intent::detect_intent;
use steps::route_regions::route_regions;
use types::ElementId;

/// Maximum tokens accepted in a single tick. Matches GLiNER2's 512-token
/// max-input minus a safety margin for special tokens, positional buffer,
/// and coref-context bytes (§11.4). Inputs above this are rejected at the
/// tick boundary — the caller chunks long inputs into multiple ticks.
pub const MAX_INPUT_TOKENS: usize = 480;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: legend <text>");
        return Ok(());
    }

    let input_text = args[1].clone();
    let _wall_clock = SystemTime::now();

    let token_count = embed::token_count(&input_text);
    if token_count > MAX_INPUT_TOKENS {
        eprintln!(
            "input too long: got {token_count} tokens, max is {MAX_INPUT_TOKENS}.\n\
             legend processes one tick at a time. chunk your input into smaller pieces \
             (each ≤{MAX_INPUT_TOKENS} tokens) and submit them as separate ticks."
        );
        std::process::exit(1);
    }

    let hg = load_seed_graph();

    let embedding = embed::embed_text(&input_text);
    let intent = detect_intent(&input_text, &embedding);
    let policy = adjust_policy(&intent, &hg.policy);
    let route = route_regions(&embedding, &hg, &policy);

    println!("intent");
    println!("  conviction       {:.3}", intent.conviction);
    println!("  prediction_error {:.3}", intent.prediction_error);
    println!("  arousal          {:.3}", intent.arousal);
    println!("  curiosity        {:.3}", intent.curiosity);

    println!("policy (adjusted)");
    println!("  default_conf           {:.3}", policy.default_conf);
    println!("  salience_multiplier    {:.3}", policy.salience_multiplier);
    println!("  leaf_vigilance         {:.3}", policy.leaf_vigilance);
    println!("  hebbian_rate           {:.3}", policy.hebbian_rate);
    println!(
        "  supersession_threshold {:.3}",
        policy.supersession_threshold
    );

    println!("embedding ({}-dim, shared with Step 1)", embedding.len());
    print!(" ");
    for v in embedding.iter().take(8) {
        print!(" {v:+.4}");
    }
    println!(" …");

    println!("seed graph");
    println!("  elements         {}", hg.elements.len());
    println!("  relations        {}", hg.relations.len());
    println!(
        "  region children of GENESIS  {}",
        hg.region_children.get(&hg.genesis).map_or(0, |v| v.len()),
    );

    println!("region routing");
    println!(
        "  thresholds (adj)    cos.descend≥{:.3}  cos.leaf≥{:.3}  M.activate≥{:.3}  var_prior={:.4}",
        policy.descend_threshold,
        policy.leaf_vigilance,
        policy.region_activation_threshold,
        policy.variance_prior,
    );

    // Display both fusion scores. Sort by cosine — the sharp signal
    // and the one driving descent ordering.
    let mut scored: Vec<(String, f32, f32, ElementId)> = route
        .all_scores
        .iter()
        .map(|rs| {
            let name = hg.elements[rs.region.0 as usize]
                .names
                .first()
                .cloned()
                .unwrap_or_else(|| format!("?{}?", rs.region.0));
            (name, rs.cosine, rs.mahalanobis, rs.region)
        })
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    let activated: HashSet<ElementId> =
        route.active_regions.iter().map(|ra| ra.region).collect();
    let descended: HashSet<ElementId> = route
        .delta
        .parent_attachments
        .iter()
        .map(|(c, _, _)| *c)
        .collect();
    // Parent-voided when best COSINE across children falls below
    // leaf_vigilance — matches route_regions' leaf gate.
    let parent_voided = !scored.is_empty()
        && scored
            .iter()
            .map(|(_, c, _, _)| *c)
            .fold(f32::NEG_INFINITY, f32::max)
            < policy.leaf_vigilance;

    println!();
    println!(
        "  {:<20} {:>8} {:>8}  status",
        "region (under GENESIS)", "cosine", "M-sim"
    );
    println!(
        "  {:-<20} {:->8} {:->8}  --------------------",
        "", "", ""
    );
    for (name, cosine, mahalanobis, id) in &scored {
        let status = if activated.contains(id) {
            "active"
        } else if descended.contains(id) {
            "descended"
        } else if parent_voided {
            "void (parent < leaf)"
        } else {
            "below descend"
        };
        println!(
            "  {name:<20} {cosine:>+8.4} {mahalanobis:>+8.4}  {status}"
        );
    }

    println!();
    println!("  active regions      {}", route.active_regions.len());
    println!(
        "  parent_attachments  {}",
        route.delta.parent_attachments.len()
    );
    println!(
        "  prototype_updates   {}",
        route.delta.prototype_updates.len()
    );
    println!("  void_count          {}", route.delta.void_count);
    if !route.uncertainty.is_empty() {
        println!("  uncertainty         {:?}", route.uncertainty);
    }

    let dump_path = Path::new("inspect/last_run.md");
    fs::create_dir_all(dump_path.parent().unwrap())?;
    let md = render::render(&hg);
    let mut file = fs::File::create(dump_path)?;
    file.write_all(md.as_bytes())?;
    println!(
        "graph dump          wrote {} ({} bytes)",
        dump_path.display(),
        md.len()
    );

    Ok(())
}
