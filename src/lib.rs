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
use steps::route_regions::{RouteResult, route_regions};
use steps::run_extractors::{ExtractionOutput, run_extractors};
use types::{ElementId, Hypergraph, Intent, Policy};

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
    let wall_clock = SystemTime::now();
    let timing = std::env::var("LEGEND_TIME").is_ok();
    let stage_at = |label: &str, mark: &mut SystemTime| {
        if timing {
            let dt = mark.elapsed().unwrap_or_default();
            eprintln!("[time] {label:<32} {:>6.1} ms", dt.as_secs_f64() * 1000.0);
            *mark = SystemTime::now();
        }
    };
    let mut mark = wall_clock;

    // Eagerly warm the heaviest singletons (tokenizer + INT8 weight
    // bundle) so the timing breakdown attributes their first-call cost
    // to "init" rather than burying it inside `run_extractors`.
    if timing {
        let _ = inference::deberta::tokenizer::BUNDLED_TOKENIZER.get_vocab_size(true);
        stage_at("init tokenizer (GLiNER)", &mut mark);
        let _ = inference::deberta::weights_int8::WeightsDebertaInt8::load_bundled();
        stage_at("init weights (GLiNER INT8)", &mut mark);
    }

    let token_count = embed::token_count(&input_text);
    stage_at("token_count", &mut mark);
    if token_count > MAX_INPUT_TOKENS {
        eprintln!(
            "input too long: got {token_count} tokens, max is {MAX_INPUT_TOKENS}.\n\
             legend processes one tick at a time. chunk your input into smaller pieces \
             (each ≤{MAX_INPUT_TOKENS} tokens) and submit them as separate ticks."
        );
        std::process::exit(1);
    }

    let hg = load_seed_graph();
    stage_at("load_seed_graph", &mut mark);
    print_seed_graph(&hg);

    let embedding = embed::embed_text(&input_text);
    stage_at("embed_text (MiniLM)", &mut mark);
    print_embedding(&embedding);

    let intent = detect_intent(&input_text, &embedding);
    stage_at("detect_intent", &mut mark);
    print_intent(&intent);

    let policy = adjust_policy(&intent, &hg.policy);
    stage_at("adjust_policy", &mut mark);
    print_policy(&policy);

    let route = route_regions(&embedding, &hg, &policy);
    stage_at("route_regions", &mut mark);
    print_routing(&hg, &policy, &route);

    let out = run_extractors(&input_text, &[], &policy, &hg, &route.active_regions);
    stage_at("run_extractors (Step 5)", &mut mark);
    print_extraction(&input_text, &out);

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

// ─── dev-time print helpers ───────────────────────────────────────────────
// These render intermediate pipeline state to stdout while the pipeline is
// under active development. Once output goes somewhere structured (UI, log
// pipeline, conformance harness, etc.), delete the entire block below along
// with the `print_*` calls in `run()`.

fn print_seed_graph(hg: &Hypergraph) {
    println!("seed graph");
    println!("  elements         {}", hg.elements.len());
    println!("  relations        {}", hg.relations.len());
    println!(
        "  region children of GENESIS  {}",
        hg.region_children.get(&hg.genesis).map_or(0, |v| v.len()),
    );
}

fn print_embedding(embedding: &[f32]) {
    println!("embedding ({}-dim, shared with Step 1)", embedding.len());
    print!(" ");
    for v in embedding.iter().take(8) {
        print!(" {v:+.4}");
    }
    println!(" …");
}

fn print_intent(intent: &Intent) {
    println!("intent");
    println!("  conviction       {:.3}", intent.conviction);
    println!("  prediction_error {:.3}", intent.prediction_error);
    println!("  arousal          {:.3}", intent.arousal);
    println!("  curiosity        {:.3}", intent.curiosity);
}

fn print_policy(policy: &Policy) {
    println!("policy (adjusted)");
    println!("  default_conf           {:.3}", policy.default_conf);
    println!("  salience_multiplier    {:.3}", policy.salience_multiplier);
    println!("  leaf_vigilance         {:.3}", policy.leaf_vigilance);
    println!("  hebbian_rate           {:.3}", policy.hebbian_rate);
    println!(
        "  supersession_threshold {:.3}",
        policy.supersession_threshold
    );
}

fn print_routing(hg: &Hypergraph, policy: &Policy, route: &RouteResult) {
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

    let activated: HashSet<ElementId> = route.active_regions.iter().map(|ra| ra.region).collect();
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
    println!("  {:-<20} {:->8} {:->8}  --------------------", "", "", "");
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
        println!("  {name:<20} {cosine:>+8.4} {mahalanobis:>+8.4}  {status}");
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
}

fn print_extraction(input_text: &str, out: &ExtractionOutput) {
    println!();
    println!("run_extractors (Step 5)");

    // Step 5a — unconditional chunks. Always populated for non-empty input.
    if out.unconditional_chunks.is_empty() {
        println!("  unconditional_chunks: (none)");
    } else {
        let phrase_count = out
            .unconditional_chunks
            .iter()
            .filter(|c| {
                matches!(
                    c.scale,
                    crate::steps::orthographic::ChunkScale::Phrase
                )
            })
            .count();
        let token_count = out.unconditional_chunks.len() - phrase_count;
        println!(
            "  unconditional_chunks  {} total  ({} phrases, {} tokens)",
            out.unconditional_chunks.len(),
            phrase_count,
            token_count
        );
        for c in &out.unconditional_chunks {
            let truncated: String = if c.text.chars().count() > 36 {
                let cut: String = c.text.chars().take(33).collect();
                format!("{cut}…")
            } else {
                c.text.clone()
            };
            println!("    {:<8} {truncated}", format!("{:?}", c.scale));
        }
    }
    println!();

    if out.instance_of.is_empty() {
        println!("  instance_of:  (none)");
    } else {
        #[allow(clippy::print_literal)]
        {
            println!(
                "  {:<24} {:<14} {:>6}  {:<10} src",
                "span", "label", "conf", "status"
            );
        }
        println!(
            "  {:-<24} {:-<14} {:->6}  {:-<10} {:-<8}",
            "", "", "", "", ""
        );
        for p in &out.instance_of {
            let truncated: String = if p.subject_text.chars().count() > 24 {
                let cut: String = p.subject_text.chars().take(21).collect();
                format!("{cut}…")
            } else {
                p.subject_text.clone()
            };
            println!(
                "  {:<24} {:<14} {:>6.3}  {:<10} {:?}",
                truncated,
                p.object_label,
                p.confidence,
                format!("{:?}", p.status),
                p.provenance,
            );
        }
    }
    if !out.relations.is_empty() {
        println!();
        println!("  relations");
        for r in &out.relations {
            let subj = &input_text[r.subject_char_start..r.subject_char_end];
            let obj = &input_text[r.object_char_start..r.object_char_end];
            println!(
                "    ({subj}) {attr} ({obj})  conf={conf:.3}  {status:?}",
                attr = r.attribute_name,
                conf = r.confidence,
                status = r.status
            );
        }
    }
    if !out.coref.is_empty() {
        println!();
        println!("  coref decisions: {}", out.coref.len());
    }
}
