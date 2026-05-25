//! Multi-turn conversation simulating an LLM-user dialogue. The
//! consumer LLM would see one `ConsciousAttentionFrame` per turn and
//! is expected to ground its reply on the frame alone. This example
//! dumps each frame in detail so the human reader can judge whether
//! it carries enough grounded data — and runs structural assertions
//! at checkpoint turns where we know what the frame *should* surface.
//!
//! Run: `cargo run --release --example conversation_session`
//!
//! Exit codes:
//! - 0  all checkpoint assertions held
//! - 1  one or more checkpoints failed (details printed to stderr)

use legend::seed::load_seed_graph;
use legend::tick_pipeline::adjust_policy::adjust_policy;
use legend::tick_pipeline::apply_region_delta::apply_region_delta;
use legend::tick_pipeline::build_relations::build_relations;
use legend::tick_pipeline::decay::focus_radius_decay;
use legend::tick_pipeline::detect_intent::detect_intent;
use legend::tick_pipeline::frame::assemble_frame;
use legend::tick_pipeline::hebbian::{derive_active_frame, hebbian_and_salience};
use legend::tick_pipeline::route_regions::route_regions;
use legend::tick_pipeline::run_extractors::run_extractors;
use legend::tick_pipeline::supersede::supersede;
use legend::types::{AttentionAction, ConsciousAttentionFrame};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Statement adding new state.
    Assertion,
    /// Statement changing prior state (should drive supersession).
    Update,
    /// Question — should retrieve, write little.
    Query,
    /// Pronoun reference to prior turn (tests coref / recent_focus).
    Coref,
    /// Multi-clause / structurally complex assertion.
    Complex,
}

struct Turn {
    text: &'static str,
    kind: Kind,
    /// Subject names (lowercase substring match against element names)
    /// that the frame's focused_relations MUST include at this turn.
    /// Empty = no assertion. Used as a "useful frame" check.
    must_focus: &'static [&'static str],
    /// True when supersession is expected to fire this turn AND
    /// `frame.history` should be non-empty.
    expect_history: bool,
    /// True when at least one supporting_claims meta should populate
    /// (i.e. base relations got minted with the source plumbed).
    expect_supporting: bool,
}

const DIALOG: &[Turn] = &[
    // ── Block 1: Introducing the project (assertions, build state) ──
    Turn {
        text: "I'm starting a new side project called Polaris.",
        kind: Kind::Assertion,
        must_focus: &["polaris"],
        expect_history: false,
        expect_supporting: true,
    },
    Turn {
        text: "Polaris is a CLI tool for managing notes.",
        kind: Kind::Assertion,
        must_focus: &["polaris"],
        expect_history: false,
        expect_supporting: true,
    },
    Turn {
        text: "I'm writing Polaris in Rust.",
        kind: Kind::Assertion,
        must_focus: &["polaris", "rust"],
        expect_history: false,
        expect_supporting: true,
    },
    Turn {
        text: "The main feature of Polaris is fuzzy search.",
        kind: Kind::Assertion,
        must_focus: &["polaris"],
        expect_history: false,
        expect_supporting: true,
    },
    Turn {
        text: "Polaris targets macOS and Linux first.",
        kind: Kind::Assertion,
        must_focus: &["polaris"],
        expect_history: false,
        expect_supporting: true,
    },
    // ── Block 2: Queries about Block 1 (recall test) ──
    Turn {
        text: "What language am I using for Polaris?",
        kind: Kind::Query,
        must_focus: &["polaris", "rust"],
        expect_history: false,
        expect_supporting: true,
    },
    Turn {
        text: "What are the platforms for Polaris?",
        kind: Kind::Query,
        must_focus: &["polaris"],
        expect_history: false,
        expect_supporting: true,
    },
    // ── Block 3: Updates (supersession test) ──
    Turn {
        // Agent-verb-patient form: "I" is the grammatical subject
        // but the EVENT target is Polaris (the patient whose state
        // is changing). The T1 verb-anchor logic in
        // `relation_patterns.rs` walks backward from " from " and
        // picks the latest NER span ending before it, which lands
        // on Polaris (not I). First event in this chain → no
        // history yet.
        text: "I switched Polaris from Rust to Go.",
        kind: Kind::Update,
        must_focus: &["polaris", "go"],
        expect_history: false,
        expect_supporting: true,
    },
    Turn {
        text: "Polaris is now written in Go.",
        kind: Kind::Update,
        must_focus: &["polaris", "go"],
        expect_history: true,
        expect_supporting: true,
    },
    Turn {
        text: "What language is Polaris in now?",
        kind: Kind::Query,
        must_focus: &["polaris", "go"],
        expect_history: false,
        expect_supporting: true,
    },
    // ── Block 4: Rename + property accumulation ──
    Turn {
        // Renames are a complex case v0 doesn't model natively (the
        // old element doesn't become the new one; both coexist).
        // Just check that Polaris stays surfaced; don't require
        // Lumen-as-a-renamed-Polaris semantics.
        text: "I renamed Polaris to Lumen.",
        kind: Kind::Update,
        must_focus: &["polaris"],
        expect_history: false,
        expect_supporting: true,
    },
    Turn {
        text: "Lumen now uses Vim-style keybindings.",
        kind: Kind::Assertion,
        must_focus: &["lumen"],
        expect_history: false,
        expect_supporting: true,
    },
    Turn {
        // Phrased as an event ("now has") so `supersede` mints a cache on
        // the first mention. Without an event-shaped phrasing, "Lumen
        // has 50 GitHub stars" lands as a binary relation only — no
        // cache — and the next turn has no prior to flip.
        text: "Lumen now has 50 GitHub stars.",
        kind: Kind::Assertion,
        must_focus: &["lumen"],
        expect_history: false,
        expect_supporting: true,
    },
    Turn {
        text: "Lumen now has 200 GitHub stars.",
        kind: Kind::Update,
        must_focus: &["lumen"],
        expect_history: true,
        expect_supporting: true,
    },
    // ── Block 5: Second entity + coref ──
    Turn {
        text: "I'm also building Beam, a server written in Go.",
        kind: Kind::Complex,
        must_focus: &["beam"],
        expect_history: false,
        expect_supporting: true,
    },
    Turn {
        text: "Beam has 500 GitHub stars.",
        kind: Kind::Assertion,
        must_focus: &["beam"],
        expect_history: false,
        expect_supporting: true,
    },
    Turn {
        text: "It also has 12 contributors.",
        kind: Kind::Coref,
        must_focus: &["beam"], // "It" should resolve to Beam
        expect_history: false,
        expect_supporting: true,
    },
    Turn {
        text: "Who works on Beam?",
        kind: Kind::Query,
        must_focus: &["beam"],
        expect_history: false,
        expect_supporting: true,
    },
    // ── Block 6: Disambiguation / multi-entity query ──
    Turn {
        text: "Both Lumen and Beam are written in Go.",
        kind: Kind::Complex,
        must_focus: &["lumen", "beam", "go"],
        expect_history: false,
        expect_supporting: true,
    },
    Turn {
        text: "What projects do I have written in Go?",
        kind: Kind::Query,
        must_focus: &["lumen", "beam", "go"],
        expect_history: false,
        expect_supporting: true,
    },
    // ── Block 7: Release planning (event-style) ──
    Turn {
        // Same event-phrasing rule as the GitHub-stars pair: the first
        // mention has to be a cache-minting event for the second to
        // have a prior to supersede.
        text: "Beam's release is now scheduled for November.",
        kind: Kind::Assertion,
        must_focus: &["beam", "november"],
        expect_history: false,
        expect_supporting: true,
    },
    Turn {
        text: "Beam's release is now January.",
        kind: Kind::Update,
        must_focus: &["beam", "january"],
        expect_history: true,
        expect_supporting: true,
    },
    Turn {
        text: "When does Beam release?",
        kind: Kind::Query,
        must_focus: &["beam", "january"],
        expect_history: false,
        expect_supporting: true,
    },
    // ── Block 8: Cross-entity recall ──
    Turn {
        text: "What do I know about Lumen?",
        kind: Kind::Query,
        must_focus: &["lumen"],
        expect_history: false,
        expect_supporting: true,
    },
    Turn {
        text: "What was Polaris?",
        kind: Kind::Query,
        must_focus: &["polaris"], // historical entity
        expect_history: false,
        expect_supporting: true,
    },
];

fn main() {
    let mut hypergraph = load_seed_graph();
    let source_id = hypergraph.genesis;

    println!("─── Legend conversation-session test ───");
    println!("  dialog turns      {}", DIALOG.len());
    println!("  seed elements     {}", hypergraph.elements.len());
    println!("  seed relations    {}", hypergraph.relations.len());
    println!();

    let mut failures: Vec<String> = Vec::new();
    let mut frames: Vec<ConsciousAttentionFrame> = Vec::new();

    let t0 = std::time::Instant::now();
    for (i, turn) in DIALOG.iter().enumerate() {
        let embedding = legend::embed::embed_text(turn.text);
        let intent = detect_intent(turn.text, &embedding);
        let policy = adjust_policy(&intent, &hypergraph.policy);
        // Inherit the prior tick's focal subject as this tick's
        // active frame. Computed before `hebbian_and_salience`'s push so the value
        // reflects history, not this tick's about-to-land entries.
        let active_frame = derive_active_frame(&hypergraph, &intent, &policy);
        let route = route_regions(&embedding, &hypergraph, &policy);
        let extraction = run_extractors(turn.text, &[], &policy, &hypergraph, &route.active_regions);
        let _ = apply_region_delta(&mut hypergraph, &route.delta, &policy);
        let built_relations = build_relations(turn.text, &mut hypergraph, &extraction, &policy, Some(source_id));
        let superseded = supersede(&mut hypergraph, &built_relations.minted_relations, &policy);
        let topical_seeds = legend::tick_pipeline::topical::topical_neighbors(&hypergraph, &embedding, 32);
        let reinforced = hebbian_and_salience(
            &mut hypergraph,
            &built_relations,
            &superseded,
            active_frame,
            &policy,
            &topical_seeds,
        );
        let _decay = focus_radius_decay(&mut hypergraph, &reinforced.reinforced, &policy);
        let frame = assemble_frame(
            turn.text,
            &hypergraph,
            &intent,
            active_frame,
            &route,
            &built_relations,
            &superseded,
            &reinforced,
            &policy,
        );

        print_turn(i + 1, turn, &frame, &built_relations, &superseded);
        check_turn(i + 1, turn, &frame, &mut failures);

        frames.push(frame);
    }
    let elapsed = t0.elapsed();

    println!();
    println!("─── final substrate ───");
    println!("  elements  {}", hypergraph.elements.len());
    println!("  relations {}", hypergraph.relations.len());

    println!();
    println!("─── aggregate ───");
    let total_focused: usize = frames.iter().map(|f| f.focused_relations.len()).sum();
    let total_supporting: usize = frames.iter().map(|f| f.supporting_claims.len()).sum();
    let total_history: usize = frames.iter().map(|f| f.history.len()).sum();
    let total_uncertainty: usize = frames.iter().map(|f| f.uncertainty.len()).sum();
    let total_next_actions: usize = frames.iter().map(|f| f.next_actions.len()).sum();
    let total_superseded: usize = frames.iter().map(|f| f.superseded.len()).sum();
    let total_writes: usize = frames.iter().map(|f| f.durable_writes.len()).sum();
    println!("  Σ focused_relations  {total_focused}");
    println!("  Σ supporting_claims  {total_supporting}");
    println!("  Σ history            {total_history}");
    println!("  Σ uncertainty        {total_uncertainty}");
    println!("  Σ next_actions       {total_next_actions}");
    println!("  Σ superseded         {total_superseded}");
    println!("  Σ durable_writes     {total_writes}");
    println!("  total                {elapsed:?}");
    println!(
        "  per turn             {:.0}ms (avg)",
        elapsed.as_secs_f64() * 1000.0 / DIALOG.len() as f64,
    );

    println!();
    if failures.is_empty() {
        println!("─── ✓ all {} checkpoints passed ───", DIALOG.len());
    } else {
        eprintln!("─── ✗ {} checkpoint failures ───", failures.len());
        for f in &failures {
            eprintln!("  • {f}");
        }
        std::process::exit(1);
    }
}

fn print_turn(
    n: usize,
    turn: &Turn,
    frame: &ConsciousAttentionFrame,
    built_relations: &legend::tick_pipeline::build_relations::MintedRelations,
    superseded: &legend::tick_pipeline::supersede::Supersession,
) {
    println!("─── turn {n:>2} [{:?}] ────────────────", turn.kind);
    println!("  in:  {:?}", turn.text);
    println!(
        "  intent: conv={:.2} pe={:.2} arous={:.2} curio={:.2}",
        frame.intent.conviction,
        frame.intent.prediction_error,
        frame.intent.arousal,
        frame.intent.curiosity,
    );
    println!(
        "  built_relations: minted_elements={} minted_relations={}",
        built_relations.minted_elements.len(),
        built_relations.minted_relations.len(),
    );
    println!(
        "  superseded: caches={} superseded={} metas={}",
        superseded.cache_relations.len(),
        superseded.superseded.len(),
        superseded.meta_relations.len(),
    );
    let active_frame_name = frame
        .active_frame
        .as_ref()
        .map(|e| e.name.clone())
        .unwrap_or_else(|| "None".to_string());
    println!(
        "  frame: focused={} supporting={} history={} current_state={} active_frame={:?} uncertainty={:?} next_actions={}",
        frame.focused_relations.len(),
        frame.supporting_claims.len(),
        frame.history.len(),
        frame.current_state.len(),
        active_frame_name,
        frame.uncertainty,
        frame.next_actions.len(),
    );

    if !frame.focused_relations.is_empty() {
        let show = frame.focused_relations.len().min(15);
        println!(
            "  focused (top {show} of {}):",
            frame.focused_relations.len()
        );
        for ra in frame.focused_relations.iter().take(show) {
            let defeasible = ra.relation.status == legend::types::RelationStatus::Defeasible;
            print_relation(&ra.relation, ra.activation, defeasible);
        }
    }
    if !frame.supporting_claims.is_empty() {
        println!("  supporting (top 3):");
        for r in frame.supporting_claims.iter().take(3) {
            print_relation(r, 0.0, false);
        }
    }
    if !frame.history.is_empty() {
        println!("  history:");
        for r in frame.history.iter().take(4) {
            print_relation(r, 0.0, false);
        }
    }
    if !frame.current_state.is_empty() {
        println!("  current_state:");
        for r in frame.current_state.iter().take(6) {
            print_relation(r, 0.0, false);
        }
    }
    if !frame.next_actions.is_empty() {
        println!("  next_actions:");
        for action in &frame.next_actions {
            match action {
                AttentionAction::EnqueueReplay { kind } => println!("    replay {kind:?}"),
                AttentionAction::FollowUpQuery(q) => println!("    follow-up: {q:?}"),
            }
        }
    }
    println!();
}

fn print_relation(r: &legend::types::ResolvedRelation, score: f32, defeasible: bool) {
    let subj = relation_subject_name(r).unwrap_or_else(|| "?".to_string());
    let (attr, val) = relation_other_slot(r).unwrap_or_default();
    let score_str = if score > 0.0 {
        format!(" act={score:.4}")
    } else {
        String::new()
    };
    let def_str = if defeasible { " [Def]" } else { "" };
    println!(
        "    R{:<5} {:<14} {:<22} → {:<14} {:?}{score_str}{def_str}",
        r.id.0,
        truncate(&subj, 14),
        truncate(&attr, 22),
        truncate(&val, 14),
        r.status,
    );
}

fn is_subject_attr(name: &str) -> bool {
    name == "subject" || name == "target"
}

fn relation_subject_name(r: &legend::types::ResolvedRelation) -> Option<String> {
    for attr in &r.attributes {
        if !is_subject_attr(&attr.name.name) {
            continue;
        }
        let arrow = if attr.name.name == "target" { "→" } else { "" };
        return match &attr.value {
            legend::types::ResolvedTerm::Element(e) => Some(format!("{arrow}{}", e.name)),
            legend::types::ResolvedTerm::Relation(rid) => Some(format!("{arrow}R{}", rid.0)),
        };
    }
    None
}

fn relation_other_slot(r: &legend::types::ResolvedRelation) -> Option<(String, String)> {
    for attr in &r.attributes {
        if is_subject_attr(&attr.name.name) {
            continue;
        }
        let val = match &attr.value {
            legend::types::ResolvedTerm::Element(e) => e.name.clone(),
            legend::types::ResolvedTerm::Relation(rid) => format!("R{}", rid.0),
        };
        return Some((attr.name.name.clone(), val));
    }
    None
}

fn check_turn(
    n: usize,
    turn: &Turn,
    frame: &ConsciousAttentionFrame,
    failures: &mut Vec<String>,
) {
    for needle in turn.must_focus {
        let lower = needle.to_ascii_lowercase();
        let hit = frame.focused_relations.iter().any(|ra| {
            ra.relation.attributes.iter().any(|a| match &a.value {
                legend::types::ResolvedTerm::Element(e) => {
                    e.name.to_ascii_lowercase().contains(&lower)
                }
                _ => false,
            })
        });
        if !hit {
            failures.push(format!(
                "turn {n}: focused_relations missing element matching {needle:?}",
            ));
        }
    }
    if turn.expect_history && frame.history.is_empty() {
        failures.push(format!(
            "turn {n}: expected history populated but it was empty",
        ));
    }
    if turn.expect_supporting && frame.supporting_claims.is_empty() {
        failures.push(format!(
            "turn {n}: expected supporting_claims populated but it was empty",
        ));
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}
