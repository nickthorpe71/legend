//! Inspector for LongMemEval question gpt4_2312f94c (Q4):
//!
//!   "Which device did I got first, the Samsung Galaxy S22 or the Dell XPS 13?"
//!   Expected answer: Samsung Galaxy S22
//!
//! Ingests the 12 user turns from the oracle dataset, ticks the
//! question, then dumps the *question tick's* attention frame in full
//! so we can see exactly what a downstream consumer would receive.
//!
//! Run: cargo run --release --example inspect_q4
//!
//! Three failure modes we are looking for:
//!   1. Frame already contains both acquisition relations with dates →
//!      gap is consumer-side (verbalization).
//!   2. Frame contains one but not the other → retrieval gap.
//!   3. Frame contains relations but dates are buried inside object
//!      strings rather than being structured comparables → substrate
//!      gap (date extraction).

use legend::seed::load_seed_graph;
use legend::steps::adjust_policy::adjust_policy;
use legend::steps::apply_region_delta::apply_region_delta;
use legend::steps::build_relations::build_relations;
use legend::steps::decay::focus_radius_decay;
use legend::steps::detect_intent::detect_intent;
use legend::steps::frame::assemble_frame;
use legend::steps::hebbian::{derive_active_frame, hebbian_and_salience};
use legend::steps::route_regions::route_regions;
use legend::steps::run_extractors::run_extractors;
use legend::steps::supersede::supersede;
use legend::types::{ConsciousAttentionFrame, Hypergraph, RelationId, Term};

const USER_TURNS: &[&str] = &[
    // session 0
    "I'm planning a trip to Hawaii and I want to make sure my new phone stays charged. Can you recommend some must-visit places in Hawaii? By the way, I recently got a new Samsung Galaxy S22 from the Best Buy store at the mall on February 20th, and I'm loving it so far.",
    "I was thinking of visiting the Haleakala National Park on Maui. Can you tell me more about the sunrise viewing at the summit? I got a good deal on my new phone, saving around $100 with a student discount, and I also traded in my old phone, iPhone 12, to get an additional $200 off, so I'm excited to capture some great shots with the phone's camera.",
    "I'm also planning to use my portable power bank, Anker PowerCore 20000, that I bought from Amazon a week before I got my new phone, to ensure my devices stay charged during the trip.",
    "Can you recommend some good hiking trails on Maui that are suitable for a beginner like me? I've been using my new phone to track my fitness goals, and I'm excited to get some exercise while enjoying the island's natural beauty.",
    "I'm also planning to visit the Road to Hana, which I've heard is a scenic drive with many waterfalls and hiking trails along the way. Can you recommend any good stops or attractions along the way?",
    "I'm planning to rent a car to drive the Road to Hana. Can you recommend any car rental companies or tips for renting a car on Maui?",
    // session 1
    "I'm planning a trip to Hawaii and I need to pack the right adapters for my devices. Can you help me figure out what kind of power adapters I'll need for my new laptop, Dell XPS 13, and my new smartphone, Samsung Galaxy S22? By the way, I pre-ordered the laptop on January 28th, and it finally arrived on February 25th after a delay from the original expected arrival date of February 11th.",
    "I was planning to bring my portable power bank, Anker PowerCore 20000, which I bought from Amazon on February 13th. It's been working great with my phone and laptop. Do you think it'll be enough to keep my devices charged during the trip, or should I consider packing another one?",
    "I'm planning to use my devices moderately during the trip, so I think the Anker PowerCore 20000 should be enough. I'm also planning to pack a travel adapter with multiple USB ports, so I can charge my devices and power bank simultaneously when I have access to a power outlet. By the way, I also have a wireless charging pad, Belkin Boost Up, that I use to charge my phone and smartwatch at home. Do you think I should bring it with me on the trip, or would it be too bulky?",
    "I think I'll leave the wireless charging pad at home. I can just use my travel adapter with multiple USB ports to charge my devices via cable. It's not a big deal to use cables for a short period of time. Can you tell me more about packing essentials for a trip to Hawaii? What are some must-haves that I shouldn't forget?",
    "I think I've got everything covered. But just to confirm, can you tell me a bit more about the weather in Hawaii? I want to make sure I'm prepared for any conditions.",
    "I think I'm all set! Thanks for the weather info. I'll make sure to pack accordingly and stay prepared for any conditions. I'm super excited for my trip to Hawaii now!",
];

const QUESTION: &str = "Which device did I got first, the Samsung Galaxy S22 or the Dell XPS 13?";

fn main() {
    let mut hg = load_seed_graph();
    let source_id = hg.genesis;

    println!("─── Ingesting 12 user turns ───");
    let t0 = std::time::Instant::now();
    for (i, text) in USER_TURNS.iter().enumerate() {
        tick(&mut hg, text, source_id);
        let preview: String = text.chars().take(70).collect();
        println!("  turn {:>2}: {} ...", i + 1, preview);
    }
    println!(
        "  ingest done in {:.2}s · elements={} relations={}",
        t0.elapsed().as_secs_f32(),
        hg.elements.len(),
        hg.relations.len(),
    );

    println!();
    println!("─── Question tick ───");
    println!("  Q: {QUESTION}");
    let frame = tick(&mut hg, QUESTION, source_id);

    dump_frame(&frame, &hg);
    audit_substrate(&hg);
}

fn tick(
    hg: &mut Hypergraph,
    text: &str,
    source_id: legend::types::ElementId,
) -> ConsciousAttentionFrame {
    let embedding = legend::embed::embed_text(text);
    let intent = detect_intent(text, &embedding);
    let policy = adjust_policy(&intent, &hg.policy);
    let active_frame = derive_active_frame(hg);
    let route = route_regions(&embedding, hg, &policy);
    let extraction = run_extractors(text, &[], &policy, hg, &route.active_regions);
    let _ = apply_region_delta(hg, &route.delta, &policy);
    let step8 = build_relations(text, hg, &extraction, &policy, Some(source_id));
    let step9 = supersede(hg, &step8.minted_relations, &policy);
    let topical_seeds = legend::steps::topical::topical_neighbors(hg, &embedding, 32);
    let step10 = hebbian_and_salience(hg, &step8, &step9, active_frame, &policy, &topical_seeds);
    let _step11 = focus_radius_decay(hg, &step10.reinforced, &policy);
    assemble_frame(
        text,
        hg,
        &intent,
        active_frame,
        &route,
        &step8,
        &step9,
        &step10,
        &policy,
    )
}

fn dump_frame(frame: &ConsciousAttentionFrame, hg: &Hypergraph) {
    println!();
    println!(
        "  intent: conv={:.2} pe={:.2} arous={:.2} curio={:.2}",
        frame.intent.conviction,
        frame.intent.prediction_error,
        frame.intent.arousal,
        frame.intent.curiosity,
    );
    let active_frame_name = frame
        .active_frame
        .and_then(|eid| hg.elements[eid.0 as usize].names.first().cloned())
        .unwrap_or_else(|| "None".to_string());
    println!(
        "  frame: focused={} supporting={} history={} current_state={} active_frame={:?} uncertainty={:?}",
        frame.focused_relations.len(),
        frame.supporting_claims.len(),
        frame.history.len(),
        frame.current_state.len(),
        active_frame_name,
        frame.uncertainty,
    );

    println!();
    println!(
        "─── focused_relations (all {}) ───",
        frame.focused_relations.len()
    );
    for ra in &frame.focused_relations {
        print_relation_full(hg, ra.relation, Some(ra.activation));
    }

    if !frame.current_state.is_empty() {
        println!();
        println!("─── current_state (all {}) ───", frame.current_state.len());
        for &rid in &frame.current_state {
            print_relation_full(hg, rid, None);
        }
    }
    if !frame.supporting_claims.is_empty() {
        println!();
        println!(
            "─── supporting_claims (all {}) ───",
            frame.supporting_claims.len()
        );
        for &rid in &frame.supporting_claims {
            print_relation_full(hg, rid, None);
        }
    }
    if !frame.history.is_empty() {
        println!();
        println!("─── history (all {}) ───", frame.history.len());
        for &rid in &frame.history {
            print_relation_full(hg, rid, None);
        }
    }
}

fn print_relation_full(hg: &Hypergraph, rid: RelationId, activation: Option<f32>) {
    let r = &hg.relations[rid.0 as usize];
    let act_str = activation
        .map(|a| format!(" act={a:.3}"))
        .unwrap_or_default();
    println!(
        "  R{:<5} [{:?}] conf={:.2}{}",
        rid.0, r.status, r.stats.confidence, act_str
    );
    for attr in &r.attributes {
        let name = hg.elements[attr.name.0 as usize]
            .names
            .first()
            .cloned()
            .unwrap_or_else(|| format!("e{}", attr.name.0));
        let val = match attr.value {
            Term::Element(eid) => hg.elements[eid.0 as usize]
                .names
                .first()
                .cloned()
                .unwrap_or_else(|| format!("e{}", eid.0)),
            Term::Relation(rid) => format!("→R{}", rid.0),
        };
        println!("      {name:<22} = {val}");
    }
}

fn audit_substrate(hg: &Hypergraph) {
    println!();
    println!("─── Audit: looking for S22 / Dell / dates in substrate ───");
    let needles = [
        "samsung", "galaxy", "s22", "dell", "xps", "13", "february", "january", "20th", "25th",
        "28th", "11th", "13th",
    ];
    let mut hits: Vec<(usize, String)> = Vec::new();
    for (i, e) in hg.elements.iter().enumerate() {
        for n in &e.names {
            let lower = n.to_ascii_lowercase();
            for needle in &needles {
                if lower.contains(needle) {
                    hits.push((i, n.clone()));
                    break;
                }
            }
        }
    }
    println!("  matching elements: {}", hits.len());
    for (i, name) in hits.iter().take(40) {
        println!("    e{i:<6} {name:?}");
    }
    if hits.len() > 40 {
        println!("    ... ({} more)", hits.len() - 40);
    }

    // Relations mentioning any of those element ids
    let element_ids: std::collections::HashSet<u32> = hits.iter().map(|(i, _)| *i as u32).collect();
    println!();
    println!("─── Relations touching any of those elements ───");
    let mut count = 0;
    for (rid_idx, r) in hg.relations.iter().enumerate() {
        let touches = r.attributes.iter().any(|a| {
            element_ids.contains(&(a.name.0))
                || matches!(a.value, Term::Element(e) if element_ids.contains(&e.0))
        });
        if touches {
            print_relation_full(hg, RelationId(rid_idx as u32), None);
            count += 1;
            if count >= 30 {
                println!("  ... (more truncated)");
                break;
            }
        }
    }
    println!("  total relations touching: {count}");
}
