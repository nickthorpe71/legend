pub mod daemon;
pub mod embed;
pub mod hebbian;
pub mod inference;
pub mod intent_classifiers;
pub mod lexical_features;
pub mod math;
pub mod merge;
pub mod persistence;
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
use std::env;
use steps::adjust_policy::adjust_policy;
use steps::apply_region_delta::{RegionDeltaApplied, apply_region_delta};
use steps::build_relations::{build_relations, print_step8};
use steps::decay::{focus_radius_decay, print_step11};
use steps::detect_intent::detect_intent;
use steps::frame::{assemble_frame, print_step12};
use steps::hebbian::{derive_active_frame, hebbian_and_salience, print_step10};
use steps::route_regions::{RouteResult, route_regions};
use steps::run_extractors::{ExtractionOutput, run_extractors};
use steps::supersede::{print_step9, supersede};
use types::{ElementId, Hypergraph, Intent, Policy};

/// Maximum tokens accepted in a single tick. Matches GLiNER2's 512-token
/// max-input minus a safety margin for special tokens, positional buffer,
/// and coref-context bytes (§11.4). Inputs above this are rejected at the
/// tick boundary — the caller chunks long inputs into multiple ticks.
pub const MAX_INPUT_TOKENS: usize = 480;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!(
            "usage: legend <text>           # run one tick (auto-starts daemon)\n\
             legend start                  # launch daemon in the background\n\
             legend stop                   # ask the daemon to exit cleanly\n\
             legend status                 # daemon pid, uptime, substrate sizes\n\
             legend init                   # set up this repo for legend (merge driver, …)"
        );
        return Ok(());
    }

    // Subcommand dispatch. Daemon verbs first, then init, then fall
    // through to the tick path. `__daemon` is the private "run as
    // the daemon process" verb; `start` wraps it with detached spawn.
    // `git-merge-driver` is invoked by git itself (registered via
    // `legend init`); users never type it.
    match args[1].as_str() {
        "__daemon" => return daemon::serve(),
        "start" => return daemon_start(),
        "stop" => return daemon_stop(),
        "status" => return daemon_status(),
        "git-merge-driver" => return git_merge_driver(&args[2..]),
        "init" => return init(),
        _ => {}
    }

    // Tick path: first positional arg is the input text; trailing
    // positional args are ignored. `args.len() >= 2` is guaranteed
    // by the usage-banner short-circuit above.
    let input_text = args[1].clone();

    // Default tick path goes through the daemon: auto-start if not
    // running, send a `Tick` request, print the response summary.
    //
    // Opt out into the in-process verbose Step-1-through-12 pipeline
    // below when:
    //   - `LEGEND_INPROC=1` (dev inspection of every intermediate step)
    //   - `LEGEND_RESET=1`  (the reset semantics are per-invocation
    //     and only the in-process path implements them — a running
    //     daemon already holds the substrate; resetting would require
    //     stopping it, which is a separate user action)
    if env::var_os("LEGEND_INPROC").is_none() && env::var_os("LEGEND_RESET").is_none() {
        return tick_via_daemon(&input_text);
    }

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

    // Load from disk if a snapshot exists, else fall back to the
    // seed binary and persist on the way out. `LEGEND_RESET=1` forces
    // a fresh load from seed (useful for tests and for wiping a
    // corrupted snapshot without `rm`-ing the file).
    let snapshot_path = persistence::default_path();
    let mut hg = if env::var_os("LEGEND_RESET").is_some() {
        load_seed_graph()
    } else {
        persistence::load_or_seed(&snapshot_path)?
    };
    stage_at("load_or_seed", &mut mark);
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

    // Inherit the prior tick's focal subject as this tick's active
    // frame. Derived from `recent_focus` before any of this tick's
    // pushes land, so the value reflects history. None on the first
    // tick of a session.
    let active_frame = derive_active_frame(&hg);

    let route = route_regions(&embedding, &hg, &policy);
    stage_at("route_regions", &mut mark);
    print_routing(&hg, &policy, &route);

    let out = run_extractors(&input_text, &[], &policy, &hg, &route.active_regions);
    stage_at("run_extractors (Step 5)", &mut mark);
    print_extraction(&input_text, &out);

    // Step 7 — commit the RegionDelta collected by Step 4. With v0
    // defaults (policy.hebbian_rate = 0.0) this is a structural no-op:
    // access_counts bump but prototype embeddings stay put.
    let applied = apply_region_delta(&mut hg, &route.delta, &policy);
    stage_at("apply_region_delta (Step 7)", &mut mark);
    print_apply_region_delta(&policy, &applied);

    // Step 8 — mint elements + relations from Step 5's proposals.
    // No `source` plumbed yet; CLI runs are sourceless (the source
    // parameter is for replay and inter-agent inputs, §11.1).
    let prior_elements = hg.elements.len();
    let prior_relations = hg.relations.len();
    let step8 = build_relations(&input_text, &mut hg, &out, &policy, None);
    stage_at("build_relations (Step 8)", &mut mark);
    print_step8(&step8, &hg, prior_elements, prior_relations);

    // Step 9 — supersede prior cache state for each event Step 8
    // minted; write the new cache + linking meta-relations. Reads
    // policy.supersession_threshold (intent-modulated by Step 2)
    // and any `intervened` meta-relations on the event.
    let prior_elements_9 = hg.elements.len();
    let prior_relations_9 = hg.relations.len();
    let step9 = supersede(&mut hg, &step8.minted_relations, &policy);
    stage_at("supersede (Step 9)", &mut mark);
    print_step9(&step9, &hg, prior_elements_9, prior_relations_9);

    // Step 10 — Hebbian + salience + promotion + recent_focus push.
    // `active_frame` is inherited from prior `recent_focus` above
    // (None on the very first tick). New entries record this frame
    // so subsequent ticks can see the binding context.
    //
    // `topical_seeds` is the top-K cosine-similar Signal elements
    // to this tick's input embedding. Adding them to the retrieval
    // set means the frame surfaces semantically related context
    // even when the input doesn't mention those entities by name —
    // critical for queries like "what was the first issue with my
    // car?" where the relevant content was about a "GPS system"
    // and shares no NER-tagged surface tokens with the query.
    let topical_seeds = steps::topical::topical_neighbors(&hg, &embedding, 32);
    let step10 = hebbian_and_salience(
        &mut hg,
        &step8,
        &step9,
        active_frame,
        &policy,
        &topical_seeds,
    );
    stage_at("hebbian + salience (Step 10)", &mut mark);
    print_step10(&step10, &hg, &policy);

    // Step 11 — focus-radius decay. Walks outward from
    // step10.reinforced via relations_by_element, decaying each
    // non-reinforced relation reached. Default policy
    // (focus_decay_radius=0 AND decay_rate=0) is a no-op gate.
    let step11 = focus_radius_decay(&mut hg, &step10.reinforced, &policy);
    stage_at("focus-radius decay (Step 11)", &mut mark);
    print_step11(&step11, &policy);

    // Step 12 — assemble the post-tick attention frame. Read-only
    // over the Hypergraph. RRF over (dense activation rank +
    // path-reinforced focus_success_count rank); status-filtered
    // (Asserted/Entailed/Defeasible pass; Superseded/Retracted
    // excluded). Maps Step 4's uncertainty signals into
    // next_actions advisories.
    let frame = assemble_frame(
        &input_text,
        &hg,
        &intent,
        active_frame,
        &route,
        &step8,
        &step9,
        &step10,
        &policy,
    );
    stage_at("assemble frame (Step 12)", &mut mark);
    print_step12(&frame, &hg);
    print_flat_frame(&frame, &hg);

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

    // Persist substrate so the next invocation continues from here.
    // Only on the success path — any earlier `?` short-circuits past
    // this point so a failed tick doesn't overwrite a clean snapshot.
    persistence::save(&hg, &snapshot_path)?;
    let snapshot_bytes = fs::metadata(&snapshot_path).map(|m| m.len()).unwrap_or(0);
    println!(
        "persisted           {} ({} bytes)",
        snapshot_path.display(),
        snapshot_bytes,
    );

    Ok(())
}

// ─── git-merge-driver entry point ─────────────────────────────────────────

/// Invoked by git when `.legend/memory.lz4` has merge conflicts.
/// Args (per git's merge-driver contract): `%O %A %B [%P]` —
/// ancestor, ours, theirs, and (optionally) the path it's resolving.
///
/// We don't use %O (the ancestor) in v0 — the merge is symmetric
/// union with conflict-resolution rules baked in, not a 3-way diff.
/// `%P` is purely informational; we accept and ignore it.
///
/// Writes the merged result to `%A` (ours's path) and exits 0 on
/// success — git treats non-zero as "conflict not resolved" and
/// leaves the user to resolve manually.
pub fn git_merge_driver(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.len() < 3 {
        return Err("usage: legend git-merge-driver %O %A %B [%P]\n\
             %O=ancestor, %A=ours, %B=theirs, %P=path (optional)"
            .into());
    }
    let _ancestor_path = &args[0];
    let ours_path = std::path::Path::new(&args[1]);
    let theirs_path = std::path::Path::new(&args[2]);
    let filename = args.get(3).map(|s| s.as_str()).unwrap_or("memory.lz4");

    eprintln!("[legend] merging {filename}");
    let mut ours = persistence::load(ours_path)?;
    let theirs = persistence::load(theirs_path)?;
    let stats = merge::merge_hypergraphs(&mut ours, &theirs)?;
    persistence::save(&ours, ours_path)?;
    eprintln!(
        "[legend] merged: +{} elements, ={} unified, +{} relations, ={} merged, +{} recent_focus",
        stats.elements_added,
        stats.elements_unified,
        stats.relations_added,
        stats.relations_merged,
        stats.recent_focus_added,
    );
    Ok(())
}

/// Set up the current git repo to use Legend. Today this only
/// registers the substrate-aware merge driver for `.legend/memory.lz4`
/// conflicts; future work will also drop the cmp/agent files (CLAUDE.md
/// integration, agent harness configs) into the repo.
///
/// Each setup step is idempotent — re-running is safe.
pub fn init() -> Result<(), Box<dyn std::error::Error>> {
    init_merge_driver()
}

/// Register Legend's merge driver for `.legend/memory.lz4`. Writes:
///   - `git config --local merge.legend.driver "<this-binary> git-merge-driver %O %A %B %P"`
///   - `.gitattributes` line `.legend/memory.lz4 merge=legend`
fn init_merge_driver() -> Result<(), Box<dyn std::error::Error>> {
    let exe = std::env::current_exe()?;
    let driver_cmd = format!("{} git-merge-driver %O %A %B %P", exe.display());
    let status = std::process::Command::new("git")
        .args(["config", "--local", "merge.legend.driver", &driver_cmd])
        .status()?;
    if !status.success() {
        return Err("git config --local failed; are you inside a git repo?".into());
    }
    println!("✓ registered merge.legend.driver = {driver_cmd}");

    let attrs_path = std::path::Path::new(".gitattributes");
    let existing = fs::read_to_string(attrs_path).unwrap_or_default();
    let rule = ".legend/memory.lz4 merge=legend";
    if !existing.lines().any(|l| l.trim() == rule) {
        let mut out = existing;
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(rule);
        out.push('\n');
        fs::write(attrs_path, out)?;
        println!("✓ added `{rule}` to .gitattributes");
    } else {
        println!("✓ .gitattributes already has `{rule}`");
    }
    Ok(())
}

// ─── Daemon subcommand wrappers + client tick path ───────────────────────

/// `legend start`: spawn the daemon detached, wait for it to become
/// reachable. Idempotent — if a daemon is already up, just reports
/// status.
fn daemon_start() -> Result<(), Box<dyn std::error::Error>> {
    if let Ok(daemon::DaemonResponse::Status {
        pid, uptime_secs, ..
    }) = daemon::round_trip(&daemon::DaemonRequest::Status)
    {
        println!("daemon already running: pid {pid} ({uptime_secs}s uptime)");
        return Ok(());
    }
    daemon::spawn_detached(std::time::Duration::from_secs(15))?;
    let resp = daemon::round_trip(&daemon::DaemonRequest::Status)?;
    if let daemon::DaemonResponse::Status { pid, .. } = resp {
        println!("daemon started: pid {pid}");
    }
    Ok(())
}

/// `legend stop`: connect, send `Stop`, daemon exits and cleans up
/// its lock + port file. Reports a friendly message if no daemon
/// is running rather than an error.
fn daemon_stop() -> Result<(), Box<dyn std::error::Error>> {
    match daemon::round_trip(&daemon::DaemonRequest::Stop) {
        Ok(daemon::DaemonResponse::Stopping) => {
            println!("daemon stopping");
            Ok(())
        }
        Ok(other) => Err(format!("unexpected response to Stop: {other:?}").into()),
        Err(_) => {
            println!("no daemon running");
            Ok(())
        }
    }
}

/// `legend status`: print pid / uptime / tick count / substrate
/// sizes if a daemon is running.
fn daemon_status() -> Result<(), Box<dyn std::error::Error>> {
    match daemon::round_trip(&daemon::DaemonRequest::Status) {
        Ok(daemon::DaemonResponse::Status {
            pid,
            uptime_secs,
            tick_count,
            elements,
            relations,
        }) => {
            println!(
                "daemon  pid={pid}  uptime={uptime_secs}s  ticks={tick_count}  \
                 elements={elements}  relations={relations}",
            );
            Ok(())
        }
        Ok(other) => Err(format!("unexpected status response: {other:?}").into()),
        Err(_) => {
            println!("no daemon running");
            Ok(())
        }
    }
}

/// Auto-start-on-tick path. Connect to a running daemon (spawn one
/// if missing), send `Tick { input }`, print the returned frame —
/// compact summary, structured frame contents, and the bench-scored
/// flat-frame view (the same string a downstream verbalizer or the
/// SubEM bench consumes).
fn tick_via_daemon(input: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut stream = daemon::connect_or_start()?;
    daemon::write_frame(
        &mut stream,
        &daemon::DaemonRequest::Tick {
            input: input.to_string(),
        },
    )?;
    let resp: daemon::DaemonResponse = daemon::read_frame(&mut stream)?;

    match resp {
        daemon::DaemonResponse::TickResult {
            frame,
            elements,
            relations,
        } => {
            // The daemon persists the substrate *before* responding, so
            // the snapshot on disk is fresh as of this tick. Loading it
            // costs ~10–50ms for the typical session-size graph and is
            // what lets us render relation IDs as `subj → attr → obj`
            // text instead of bare counts. Skipped silently on load
            // failure — the frame counts already printed are still
            // useful as a "did the tick succeed" signal.
            let hg = persistence::load(&persistence::default_path()).ok();
            print_tick_summary(&frame, elements, relations);
            if let Some(hg) = hg.as_ref() {
                print_frame_contents(&frame, hg);
                print_flat_frame(&frame, hg);
            }
            Ok(())
        }
        daemon::DaemonResponse::Error { message } => Err(message.into()),
        other => Err(format!("unexpected tick response: {other:?}").into()),
    }
}

/// Dump the bench-equivalent flat-frame view after the structured
/// render. Same content the bench harness scores SubEM against and
/// the eventual LLM verbalizer will consume — formatted with section
/// headers + per-relation grouping so a human can actually read it.
fn print_flat_frame(frame: &types::ConsciousAttentionFrame, hg: &Hypergraph) {
    println!();
    print!("{}", render::render_flat_frame_annotated(frame, hg));
}

/// Header block: intent, substrate sizes, frame bucket counts,
/// uncertainty signals, next actions. Always renderable from the
/// daemon response alone — no substrate needed.
fn print_tick_summary(frame: &types::ConsciousAttentionFrame, elements: usize, relations: usize) {
    println!(
        "tick {} \"{}\"",
        frame.tick.0,
        truncate(&frame.input_echo, 60)
    );
    println!(
        "  intent  conv={:.2}  pe={:.2}  arous={:.2}  curio={:.2}",
        frame.intent.conviction,
        frame.intent.prediction_error,
        frame.intent.arousal,
        frame.intent.curiosity,
    );
    println!(
        "  substrate  elements={elements}  relations={relations}  active_regions={}",
        frame.active_regions.len(),
    );
    println!(
        "  frame  focused={}  supporting={}  history={}  current_state={}  uncertainty={:?}",
        frame.focused_relations.len(),
        frame.supporting_claims.len(),
        frame.history.len(),
        frame.current_state.len(),
        frame.uncertainty,
    );
    if !frame.next_actions.is_empty() {
        println!("  next_actions:");
        for a in &frame.next_actions {
            match a {
                types::AttentionAction::EnqueueReplay { kind } => {
                    println!("    EnqueueReplay {{ kind: {kind:?} }}");
                }
                types::AttentionAction::FollowUpQuery(t) => {
                    println!("    FollowUpQuery({t:?})");
                }
            }
        }
    }
}

/// Per-relation render of every bucket the frame populated. Needs
/// the substrate handy to resolve relation IDs into the element
/// names that make the output readable.
fn print_frame_contents(frame: &types::ConsciousAttentionFrame, hg: &Hypergraph) {
    if !frame.focused_relations.is_empty() {
        println!("  focused:");
        for ra in &frame.focused_relations {
            print_relation_line(hg, ra.relation, Some(ra.activation));
        }
    }
    if !frame.current_state.is_empty() {
        println!("  current_state:");
        for &rid in &frame.current_state {
            print_relation_line(hg, rid, None);
        }
    }
    if !frame.supporting_claims.is_empty() {
        println!("  supporting:");
        for &rid in &frame.supporting_claims {
            print_relation_line(hg, rid, None);
        }
    }
    if !frame.history.is_empty() {
        println!("  history:");
        for &rid in &frame.history {
            print_relation_line(hg, rid, None);
        }
    }
}

/// One-line render of a relation: subject → attribute → object plus
/// status and (optionally) activation score. Subject / attribute /
/// object are resolved from the relation's attribute list against
/// the substrate; missing names degrade to `eN` ID strings rather
/// than panic.
fn print_relation_line(hg: &Hypergraph, rid: types::RelationId, activation: Option<f32>) {
    let r = &hg.relations[rid.0 as usize];
    let mut subject: Option<String> = None;
    let mut other: Option<(String, String)> = None;
    for attr in &r.attributes {
        let name = element_name(hg, attr.name);
        let value = match attr.value {
            types::Term::Element(eid) => element_name(hg, eid),
            types::Term::Relation(rid) => format!("→R{}", rid.0),
        };
        if attr.name == hg.subject_attr || attr.name == hg.target_attr {
            subject = Some(value);
        } else if other.is_none() {
            other = Some((name, value));
        }
    }
    let subject = subject.unwrap_or_else(|| "?".to_string());
    let (attr_name, attr_value) = other.unwrap_or_else(|| ("?".to_string(), "?".to_string()));
    let act = activation
        .map(|a| format!(" act={a:.3}"))
        .unwrap_or_default();
    println!(
        "    R{:<5} [{:?}] conf={:.2}{act}  {subject} → {attr_name} → {attr_value}",
        rid.0, r.status, r.stats.confidence,
    );
}

fn element_name(hg: &Hypergraph, eid: types::ElementId) -> String {
    hg.elements
        .get(eid.0 as usize)
        .and_then(|e| e.names.first().cloned())
        .unwrap_or_else(|| format!("e{}", eid.0))
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
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
    // Branch is unrouted when best COSINE across children falls below
    // leaf_vigilance — matches route_regions' leaf gate. (Distinct from
    // the polarity=Void semantic class — this is a routing failure.)
    let branch_unrouted = !scored.is_empty()
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
        } else if branch_unrouted {
            "unrouted (parent < leaf)"
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
    println!("  unrouted_count      {}", route.delta.unrouted_count);
    if !route.uncertainty.is_empty() {
        println!("  uncertainty         {:?}", route.uncertainty);
    }
}

fn print_apply_region_delta(policy: &Policy, applied: &RegionDeltaApplied) {
    println!();
    println!("apply_region_delta (Step 7)");
    println!("  prototype access bumps  {}", applied.touched);
    if applied.drifted > 0 {
        println!(
            "  prototypes drifted      {}  (lr = plasticity × hebbian_rate = {:.4})",
            applied.drifted, policy.hebbian_rate,
        );
    } else {
        println!(
            "  prototypes drifted      0  (lr = 0 at hebbian_rate {:.3} — no-op)",
            policy.hebbian_rate,
        );
    }
}

fn print_extraction(input_text: &str, out: &ExtractionOutput) {
    println!();
    println!("run_extractors (Step 5)");

    // Novelty branch — surface chunks. Always populated for non-empty input.
    if out.novelty.chunks.is_empty() {
        println!("  novelty.chunks: (none)");
    } else {
        use crate::steps::orthographic::ChunkScale;
        let mut n_phrases = 0usize;
        let mut n_tokens = 0usize;
        for c in &out.novelty.chunks {
            match c.scale {
                ChunkScale::Phrase => n_phrases += 1,
                ChunkScale::Token => n_tokens += 1,
            }
        }
        println!(
            "  novelty.chunks  {} total  ({n_phrases} phrases, {n_tokens} tokens)",
            out.novelty.chunks.len(),
        );
        for c in &out.novelty.chunks {
            let truncated: String = if c.text.chars().count() > 36 {
                let cut: String = c.text.chars().take(33).collect();
                format!("{cut}…")
            } else {
                c.text.clone()
            };
            println!("    {:<12} {truncated}", format!("{:?}", c.scale));
        }
    }
    println!();

    if out.known.instance_of.is_empty() {
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
        for p in &out.known.instance_of {
            let subj_text = &input_text[p.subject_char_start..p.subject_char_end];
            let truncated: String = if subj_text.chars().count() > 24 {
                let cut: String = subj_text.chars().take(21).collect();
                format!("{cut}…")
            } else {
                subj_text.to_string()
            };
            let label = match &p.object {
                crate::steps::relation_patterns::ObjectRef::Label(l) => l.as_str(),
                crate::steps::relation_patterns::ObjectRef::Span { .. } => "(span)",
            };
            println!(
                "  {:<24} {:<14} {:>6.3}  {:<10} {:?}",
                truncated,
                label,
                p.confidence,
                format!("{:?}", p.status),
                p.source,
            );
        }
    }
    if !out.known.relations.is_empty() {
        println!();
        println!("  relations");
        for r in &out.known.relations {
            let subj = &input_text[r.subject_char_start..r.subject_char_end];
            let obj = match &r.object {
                crate::steps::relation_patterns::ObjectRef::Span {
                    char_start,
                    char_end,
                } => input_text[*char_start..*char_end].to_string(),
                crate::steps::relation_patterns::ObjectRef::Label(l) => format!("[{l}]"),
            };
            println!(
                "    ({subj}) {attr} ({obj})  conf={conf:.3}  {status:?}",
                attr = r.attribute_name,
                conf = r.confidence,
                status = r.status
            );
        }
    }
    if !out.known.coref.is_empty() {
        println!();
        println!("  coref decisions: {}", out.known.coref.len());
    }
    if !out.novelty.relations.is_empty() {
        println!();
        println!("  novelty.relations  {}", out.novelty.relations.len());
        for r in &out.novelty.relations {
            let subj = &input_text[r.subject_char_start..r.subject_char_end];
            let obj = match &r.object {
                crate::steps::relation_patterns::ObjectRef::Span {
                    char_start,
                    char_end,
                } => input_text[*char_start..*char_end].to_string(),
                crate::steps::relation_patterns::ObjectRef::Label(l) => format!("[{l}]"),
            };
            println!(
                "    ({subj}) [{attr}] ({obj})  conf={conf:.3}",
                attr = r.attribute_name,
                conf = r.confidence,
            );
        }
    }
}
