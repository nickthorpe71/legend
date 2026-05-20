//! Drive 100 diverse inputs through the v0 pipeline in-memory and
//! summarize how the hypergraph + attention frames evolved. No
//! persistence — every run starts from the YAML seed.
//!
//! Run: `cargo run --release --example stress_100_ticks`

use legend::seed::load_seed_graph;
use legend::steps::adjust_policy::adjust_policy;
use legend::steps::apply_region_delta::apply_region_delta;
use legend::steps::build_relations::build_relations;
use legend::steps::decay::focus_radius_decay;
use legend::steps::detect_intent::detect_intent;
use legend::steps::frame::assemble_frame;
use legend::steps::hebbian::hebbian_and_salience;
use legend::steps::route_regions::route_regions;
use legend::steps::run_extractors::run_extractors;
use legend::steps::supersede::supersede;
use legend::types::{ConsciousAttentionFrame, Hypergraph, RelationStatus};

/// 100 ticks spanning recurrence, supersession, novelty, questions,
/// and intent variety. Roughly grouped so we can see how repeated
/// mentions accumulate support_count and how property changes drive
/// supersession.
const INPUTS: &[&str] = &[
    // ── Block A: recurring entity (Alice) — repeats build support ──
    "Alice is a software engineer.",
    "Alice works at a small startup in Seattle.",
    "Alice lives in Seattle.",
    "Alice enjoys hiking on weekends.",
    "Alice has a dog named Pixel.",
    "Pixel is a golden retriever.",
    "Alice and Pixel went to the park.",
    "Alice writes Rust professionally.",
    "Alice is learning Japanese.",
    "Alice mentors junior engineers.",
    // ── Block B: property changes on Bob — exercises supersession ──
    "Bob lives in Boston.",
    "Bob moved to Austin last month.",
    "Bob now lives in Austin.",
    "Bob's favorite color is blue.",
    "Bob's favorite color is now green.",
    "Bob's title is engineer.",
    "Bob was promoted to senior engineer.",
    "Bob is now a staff engineer.",
    "Bob earns 150k per year.",
    "Bob's salary increased to 175k.",
    // ── Block C: novel entities — exercises mint paths ──
    "The Voyager probe entered interstellar space.",
    "Neutrinos rarely interact with matter.",
    "Quaternions represent 3D rotations.",
    "The mitochondria is the powerhouse of the cell.",
    "Photosynthesis converts light into chemical energy.",
    "Tectonic plates move a few centimeters per year.",
    "The Roman Empire fell in 476 AD.",
    "Mount Everest is the tallest mountain on Earth.",
    "Antarctica is the coldest continent.",
    "The Pacific Ocean is the largest ocean.",
    // ── Block D: relationships and events ──
    "Carol met David at a conference.",
    "Carol and David started a company together.",
    "Their company is called Lumen Robotics.",
    "Lumen Robotics raised a seed round.",
    "Lumen Robotics has 12 employees.",
    "David is the CTO of Lumen Robotics.",
    "Carol is the CEO of Lumen Robotics.",
    "Lumen Robotics released its first product.",
    "The product is named Beam.",
    "Beam was launched in March.",
    // ── Block E: questions — different intent profile ──
    "What does Alice do for work?",
    "Where does Bob live now?",
    "Who founded Lumen Robotics?",
    "When was Beam launched?",
    "How many employees does Lumen Robotics have?",
    "What is Pixel?",
    "Why did Bob move to Austin?",
    "Which language is Alice learning?",
    "What is Carol's role?",
    "How fast do tectonic plates move?",
    // ── Block F: temporal phrases ──
    "Tomorrow I have a dentist appointment.",
    "Yesterday was a long day.",
    "Next week is Thanksgiving.",
    "The meeting is on Tuesday.",
    "We launched the product last March.",
    "She graduated in 2018.",
    "The deadline is end of quarter.",
    "I'll see you at 3pm on Friday.",
    "The conference runs from Monday to Wednesday.",
    "Bob's birthday is in June.",
    // ── Block G: negations and contradictions ──
    "Alice does not work at Lumen Robotics.",
    "Bob is not the CEO.",
    "Pixel is not a cat.",
    "Lumen Robotics is not based in Boston.",
    "Carol does not live in Seattle.",
    "David is not new to robotics.",
    "Beam is not yet profitable.",
    "The Voyager probe is not in our solar system anymore.",
    "Mount Everest is not the largest mountain by volume.",
    "Neutrinos do not have an electric charge.",
    // ── Block H: longer claims / multi-clause ──
    "Alice, who works in Seattle, is hiking Mount Rainier this weekend.",
    "Bob moved from Boston to Austin to be closer to his family.",
    "Lumen Robotics, founded by Carol and David, raised 5 million dollars.",
    "Beam, the company's first product, ships next quarter.",
    "Pixel, a golden retriever, loves playing fetch in the park.",
    "Quaternions, used in computer graphics, avoid gimbal lock.",
    "The Voyager probe, launched in 1977, carries a golden record.",
    "Mitochondria, organelles inside cells, generate ATP through cellular respiration.",
    "Mount Everest, located in the Himalayas, stands 8848 meters tall.",
    "Photosynthesis, performed by plants and algae, sustains nearly all life on Earth.",
    // ── Block I: pronouns / coref pressure ──
    "Alice picked up Pixel. She took him for a walk.",
    "Bob called Carol. He asked about the product launch.",
    "David finished the code review. He merged it this morning.",
    "Carol opened her laptop. She started reviewing the deck.",
    "The team met yesterday. They discussed the roadmap.",
    "The Voyager probe sent a signal. It took 22 hours to arrive.",
    "Lumen Robotics shipped Beam. They sold 200 units in the first week.",
    "Alice texted Bob. He responded immediately.",
    "Pixel ran outside. He chased a squirrel up a tree.",
    "Beam impressed the reviewers. They gave it a 9/10.",
    // ── Block J: short / sparse / edge ──
    "Hello.",
    "Yes.",
    "Maybe.",
    "Rust is fast.",
    "Cats sleep.",
    "Time flies.",
    "Stars shine.",
    "Code compiles.",
    "Tests pass.",
    "Ship it.",
];

fn main() {
    assert_eq!(INPUTS.len(), 100, "expected exactly 100 inputs");

    let mut hg = load_seed_graph();
    let seed_elements = hg.elements.len();
    let seed_relations = hg.relations.len();
    // Use the GENESIS element as the source for every tick — without
    // a real ingest pipeline we just need *some* element ID so each
    // base relation gets a `source` meta and `supporting_claims`
    // populates in the frame.
    let source_id = hg.genesis;

    println!("─── seed ───");
    println!("  elements  {seed_elements}");
    println!("  relations {seed_relations}");
    println!();

    // Per-tick observability we want at the end.
    let mut frames: Vec<ConsciousAttentionFrame> = Vec::with_capacity(INPUTS.len());
    let mut elements_history: Vec<usize> = Vec::with_capacity(INPUTS.len());
    let mut relations_history: Vec<usize> = Vec::with_capacity(INPUTS.len());
    let mut superseded_per_tick: Vec<usize> = Vec::with_capacity(INPUTS.len());
    let mut reinforced_per_tick: Vec<usize> = Vec::with_capacity(INPUTS.len());
    let mut promoted_per_tick: Vec<usize> = Vec::with_capacity(INPUTS.len());
    let mut focused_per_tick: Vec<usize> = Vec::with_capacity(INPUTS.len());
    let mut cache_minted_per_tick: Vec<usize> = Vec::with_capacity(INPUTS.len());

    let t0 = std::time::Instant::now();
    for (i, input_text) in INPUTS.iter().enumerate() {
        let embedding = legend::embed::embed_text(input_text);
        let intent = detect_intent(input_text, &embedding);
        let policy = adjust_policy(&intent, &hg.policy);
        let route = route_regions(&embedding, &hg, &policy);
        let extraction = run_extractors(input_text, &[], &policy, &hg, &route.active_regions);
        let _ = apply_region_delta(&mut hg, &route.delta, &policy);
        let step8 = build_relations(input_text, &mut hg, &extraction, &policy, Some(source_id));
        let step9 = supersede(&mut hg, &step8.minted_relations, &policy);
        let step10 = hebbian_and_salience(&mut hg, &step8, &step9, None, &policy, &[]);
        let _step11 = focus_radius_decay(&mut hg, &step10.reinforced, &policy);
        let frame = assemble_frame(
            input_text, &hg, &intent, None, &route, &step8, &step9, &step10, &policy,
        );

        elements_history.push(hg.elements.len());
        relations_history.push(hg.relations.len());
        superseded_per_tick.push(step9.superseded.len());
        reinforced_per_tick.push(step10.reinforced.len());
        promoted_per_tick.push(step10.promoted.len());
        focused_per_tick.push(frame.focused_relations.len());
        cache_minted_per_tick.push(step9.cache_relations.len());

        let force_print = i < 3 || i == 9 || i == 19 || i == 49 || i == 99;
        let interesting = !step9.superseded.is_empty()
            || !step10.promoted.is_empty()
            || !step9.cache_relations.is_empty();
        if force_print || interesting {
            println!(
                "tick {:>3}: el={:<5} rel={:<5} rein={:<3} cache={:<2} super={:<2} prom={:<2} foc={:<3}  | \"{}\"",
                i + 1,
                hg.elements.len(),
                hg.relations.len(),
                step10.reinforced.len(),
                step9.cache_relations.len(),
                step9.superseded.len(),
                step10.promoted.len(),
                frame.focused_relations.len(),
                truncate(input_text, 50),
            );
        }

        frames.push(frame);
    }
    let elapsed = t0.elapsed();

    println!();
    println!("─── final substrate ───");
    println!(
        "  elements  {:>6}  (Δ {:+})",
        hg.elements.len(),
        hg.elements.len() as isize - seed_elements as isize,
    );
    println!(
        "  relations {:>6}  (Δ {:+})",
        hg.relations.len(),
        hg.relations.len() as isize - seed_relations as isize,
    );

    // Relation status distribution.
    let mut asserted = 0usize;
    let mut entailed = 0usize;
    let mut defeasible = 0usize;
    let mut superseded = 0usize;
    let mut retracted = 0usize;
    for r in &hg.relations {
        match r.status {
            RelationStatus::Asserted => asserted += 1,
            RelationStatus::Entailed => entailed += 1,
            RelationStatus::Defeasible => defeasible += 1,
            RelationStatus::Superseded => superseded += 1,
            RelationStatus::Retracted => retracted += 1,
        }
    }
    println!();
    println!("─── relation status distribution ───");
    println!("  Asserted   {asserted}");
    println!("  Entailed   {entailed}");
    println!("  Defeasible {defeasible}");
    println!("  Superseded {superseded}");
    println!("  Retracted  {retracted}");

    // Support-count concentration: top entities by Σ support_count over
    // all relations whose subject is that element.
    println!();
    println!("─── top 10 most-supported elements ───");
    print_top_supported(&hg, 10);

    println!();
    println!("─── per-tick aggregates ───");
    let total_reinforced: usize = reinforced_per_tick.iter().sum();
    let total_superseded: usize = superseded_per_tick.iter().sum();
    let total_promoted: usize = promoted_per_tick.iter().sum();
    let total_focused: usize = focused_per_tick.iter().sum();
    let total_cache: usize = cache_minted_per_tick.iter().sum();
    let avg_focused = total_focused as f32 / INPUTS.len() as f32;
    let max_focused = focused_per_tick.iter().copied().max().unwrap_or(0);
    println!("  Σ reinforced   {total_reinforced}");
    println!("  Σ cache mints  {total_cache}");
    println!("  Σ superseded   {total_superseded}");
    println!("  Σ promoted     {total_promoted}");
    println!("  Σ focused      {total_focused}");
    println!("  avg focused/tick   {avg_focused:.2}");
    println!("  max focused on a tick   {max_focused}");

    println!();
    println!("─── attention-frame samples ───");
    for &i in &[0usize, 12, 49, 99] {
        let f = &frames[i];
        println!("  tick {:>3} \"{}\"", i + 1, truncate(INPUTS[i], 60),);
        println!(
            "    intent: conv={:.2} pe={:.2} arous={:.2} curio={:.2}",
            f.intent.conviction, f.intent.prediction_error, f.intent.arousal, f.intent.curiosity,
        );
        println!(
            "    focused={:<3} supporting={:<3} history={:<3} superseded={:<2} writes={:<3}",
            f.focused_relations.len(),
            f.supporting_claims.len(),
            f.history.len(),
            f.superseded.len(),
            f.durable_writes.len(),
        );
        let top_k = 3.min(f.focused_relations.len());
        for ra in f.focused_relations.iter().take(top_k) {
            let r = &hg.relations[ra.relation.0 as usize];
            let subj = r
                .attributes
                .iter()
                .find(|a| a.name == hg.subject_attr)
                .and_then(|a| match a.value {
                    legend::types::Term::Element(e) => {
                        hg.elements[e.0 as usize].names.first().cloned()
                    }
                    _ => None,
                })
                .unwrap_or_default();
            println!(
                "      R{:<5} {:<22} act={:.3}  status={:?}  defeasible={}",
                ra.relation.0,
                truncate(&subj, 22),
                ra.activation,
                r.status,
                ra.is_defeasible,
            );
        }
    }

    println!();
    println!("─── timing ───");
    println!("  total       {:?}", elapsed);
    println!(
        "  per tick    {:.2}ms (avg)",
        elapsed.as_secs_f64() * 1000.0 / INPUTS.len() as f64,
    );
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

fn print_top_supported(hg: &Hypergraph, k: usize) {
    let mut totals: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    for r in &hg.relations {
        if !matches!(
            r.status,
            RelationStatus::Asserted | RelationStatus::Entailed | RelationStatus::Defeasible
        ) {
            continue;
        }
        if let Some(attr) = r.attributes.iter().find(|a| a.name == hg.subject_attr)
            && let legend::types::Term::Element(eid) = attr.value
        {
            *totals.entry(eid.0).or_insert(0) += r.stats.support_count;
        }
    }
    let mut sorted: Vec<(u32, u32)> = totals.into_iter().collect();
    sorted.sort_by_key(|b| std::cmp::Reverse(b.1));
    for (eid, sum) in sorted.into_iter().take(k) {
        let name = hg.elements[eid as usize]
            .names
            .first()
            .cloned()
            .unwrap_or_else(|| format!("E{eid}"));
        println!("  {:<28} Σsupport={}", truncate(&name, 28), sum);
    }
}
