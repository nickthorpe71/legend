//! End-to-end test for the git-merge-driver subcommand. Simulates
//! exactly what git does when it hits a `.legend/memory.lz4`
//! conflict: invokes the binary with `%O %A %B %P` paths, expects
//! the merged result written back to `%A`, exit code 0.

use legend::persistence;
use legend::seed::load_seed_graph;
use legend::tick_pipeline::build_relations::build_relations;
use legend::tick_pipeline::decay::focus_radius_decay;
use legend::tick_pipeline::hebbian::hebbian_and_salience;
use legend::tick_pipeline::run_extractors::run_extractors;
use legend::tick_pipeline::supersede::supersede;
use legend::types::{Hypergraph, Policy};

use std::fs;
use std::path::PathBuf;
use std::process;

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "legend_merge_driver_{}_{}_{}",
        label,
        process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn tick(hypergraph: &mut Hypergraph, text: &str) {
    let policy = Policy::default();
    let ext = run_extractors(text, &[], &policy, hypergraph, &[]);
    let built_relations = build_relations(text, hypergraph, &ext, &policy, None);
    let superseded = supersede(hypergraph, &built_relations.minted_relations, &policy);
    let reinforced = hebbian_and_salience(hypergraph, &built_relations, &superseded, None, &policy, &[]);
    let _ = focus_radius_decay(hypergraph, &reinforced.reinforced, &policy);
}

#[test]
fn git_merge_driver_unifies_two_branches_via_subprocess() {
    let dir = temp_dir("subprocess_unify");

    // Build three snapshots: ancestor (just seed), ours (seed +
    // tick about Polaris/Rust), theirs (seed + tick about Beam/Go).
    let ancestor = dir.join("ancestor.lz4");
    let ours = dir.join("ours.lz4");
    let theirs = dir.join("theirs.lz4");

    let hypergraph_ancestor = load_seed_graph();
    persistence::save(&hypergraph_ancestor, &ancestor).expect("save ancestor");

    let mut hypergraph_ours = load_seed_graph();
    tick(&mut hypergraph_ours, "Polaris is written in Rust.");
    let ours_elements = hypergraph_ours.elements.len();
    let ours_relations = hypergraph_ours.relations.len();
    persistence::save(&hypergraph_ours, &ours).expect("save ours");

    let mut hypergraph_theirs = load_seed_graph();
    tick(&mut hypergraph_theirs, "Beam is a Go server.");
    persistence::save(&hypergraph_theirs, &theirs).expect("save theirs");

    // Invoke the merge driver exactly as git would.
    let bin = env!("CARGO_BIN_EXE_legend");
    let output = std::process::Command::new(bin)
        .arg("git-merge-driver")
        .arg(&ancestor)
        .arg(&ours)
        .arg(&theirs)
        .arg("memory.lz4")
        .output()
        .expect("invoke merge driver");
    assert!(
        output.status.success(),
        "merge driver exited non-zero. stderr:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("[legend] merged:"),
        "merge driver should print summary; stderr was:\n{stderr}"
    );

    // Reload the merged result and verify both Polaris (from ours)
    // and Beam (from theirs) are present.
    let merged = persistence::load(&ours).expect("load merged");
    assert!(
        merged.by_name.contains_key("Polaris"),
        "merged graph missing Polaris from ours"
    );
    assert!(
        merged.by_name.contains_key("Beam"),
        "merged graph missing Beam from theirs"
    );
    // Substrate grew: theirs's elements got added to ours.
    assert!(
        merged.elements.len() >= ours_elements,
        "merged element count must be >= ours: {} vs {}",
        merged.elements.len(),
        ours_elements,
    );
    assert!(merged.relations.len() >= ours_relations);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn git_merge_driver_dedupes_same_entity_minted_on_both_branches() {
    // Both branches independently mint "Polaris" with the same name
    // and polarity. After merge, ours should have ONE Polaris (not
    // two). The merge driver's name-based remap collapses them.
    let dir = temp_dir("subprocess_dedup");
    let ancestor = dir.join("ancestor.lz4");
    let ours = dir.join("ours.lz4");
    let theirs = dir.join("theirs.lz4");

    persistence::save(&load_seed_graph(), &ancestor).expect("save ancestor");

    let mut hypergraph_ours = load_seed_graph();
    tick(&mut hypergraph_ours, "Polaris is a project.");
    persistence::save(&hypergraph_ours, &ours).expect("save ours");

    let mut hypergraph_theirs = load_seed_graph();
    tick(&mut hypergraph_theirs, "Polaris is a project.");
    persistence::save(&hypergraph_theirs, &theirs).expect("save theirs");

    let bin = env!("CARGO_BIN_EXE_legend");
    let output = std::process::Command::new(bin)
        .arg("git-merge-driver")
        .arg(&ancestor)
        .arg(&ours)
        .arg(&theirs)
        .arg("memory.lz4")
        .output()
        .expect("invoke merge driver");
    assert!(output.status.success());

    let merged = persistence::load(&ours).expect("load merged");
    let polaris_count = merged
        .elements
        .iter()
        .filter(|e| e.names.first().map(|n| n == "Polaris").unwrap_or(false))
        .count();
    assert_eq!(
        polaris_count, 1,
        "merge driver should dedup the duplicate Polaris elements; got {polaris_count}",
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn git_merge_driver_handles_meta_relations_correctly() {
    // Both branches drive a supersession event (which mints metas
    // referencing the cache relation). After merge, the meta links
    // must point at the unified cache, not at orphaned IDs.
    let dir = temp_dir("subprocess_metas");
    let ancestor = dir.join("ancestor.lz4");
    let ours = dir.join("ours.lz4");
    let theirs = dir.join("theirs.lz4");

    persistence::save(&load_seed_graph(), &ancestor).expect("save ancestor");

    let mut hypergraph_ours = load_seed_graph();
    tick(&mut hypergraph_ours, "Polaris switched from Rust to Go.");
    persistence::save(&hypergraph_ours, &ours).expect("save ours");

    let mut hypergraph_theirs = load_seed_graph();
    tick(&mut hypergraph_theirs, "Beam switched from Python to Rust.");
    persistence::save(&hypergraph_theirs, &theirs).expect("save theirs");

    let bin = env!("CARGO_BIN_EXE_legend");
    let output = std::process::Command::new(bin)
        .arg("git-merge-driver")
        .arg(&ancestor)
        .arg(&ours)
        .arg(&theirs)
        .arg("memory.lz4")
        .output()
        .expect("invoke merge driver");
    assert!(
        output.status.success(),
        "merge driver failed: stderr=\n{}",
        String::from_utf8_lossy(&output.stderr),
    );

    let merged = persistence::load(&ours).expect("load merged");
    // Every Term::Relation reference must point at a valid index.
    for r in &merged.relations {
        for a in &r.attributes {
            if let legend::types::Term::Relation(rid) = a.value {
                assert!(
                    (rid.0 as usize) < merged.relations.len(),
                    "dangling Term::Relation({}) — must be < {}",
                    rid.0,
                    merged.relations.len(),
                );
            }
        }
    }
    // Both Polaris and Beam should be present.
    assert!(merged.by_name.contains_key("Polaris"));
    assert!(merged.by_name.contains_key("Beam"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn git_merge_driver_rejects_seed_drift_loudly() {
    // If theirs's snapshot was written against a different seed
    // binary, the persistence loader will refuse it with SeedDrift
    // BEFORE merge even starts. Verify that error path.
    let dir = temp_dir("subprocess_drift");
    let ancestor = dir.join("ancestor.lz4");
    let ours = dir.join("ours.lz4");
    let theirs = dir.join("theirs.lz4");

    persistence::save(&load_seed_graph(), &ancestor).expect("save ancestor");
    persistence::save(&load_seed_graph(), &ours).expect("save ours");

    // Hand-craft a snapshot with a tampered seed fingerprint.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"LEGEND01");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0xAB; 8]); // tampered fingerprint
    bytes.extend_from_slice(&[0u8; 32]);
    fs::write(&theirs, &bytes).unwrap();

    let bin = env!("CARGO_BIN_EXE_legend");
    let output = std::process::Command::new(bin)
        .arg("git-merge-driver")
        .arg(&ancestor)
        .arg(&ours)
        .arg(&theirs)
        .arg("memory.lz4")
        .output()
        .expect("invoke merge driver");
    assert!(
        !output.status.success(),
        "merge driver should exit non-zero on seed drift"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.to_lowercase().contains("seed") || stderr.to_lowercase().contains("drift"),
        "stderr should mention seed drift; got:\n{stderr}"
    );

    let _ = fs::remove_dir_all(&dir);
}

// ─── Real git invocation ────────────────────────────────────────────
//
// The four subprocess tests above invoke `legend git-merge-driver`
// directly with hand-crafted %O %A %B %P paths. They prove the merge
// function and its CLI work. They do NOT prove that git actually
// invokes the driver in response to a conflict on `.legend/memory.lz4`.
//
// These two tests run real `git merge` operations against a
// purpose-built temp repo. If the `.gitattributes` rule, the
// `merge.legend.driver` config, and our CLI dispatch all agree, git
// auto-resolves the conflict and leaves a clean merge commit. If
// anything is misconfigured, git falls back to a binary conflict
// and the test fails.

/// Run `git` in `dir` and assert it succeeds.
fn run_git(dir: &std::path::Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .expect("git invocation");
    assert!(
        output.status.success(),
        "git {args:?} failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Run `git` in `dir` and return the full output without asserting.
fn run_git_output(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    std::process::Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .expect("git invocation")
}

/// Initialize a fresh repo with the merge driver wired up. Returns
/// the repo dir. Caller is responsible for cleanup.
fn init_repo_with_driver(label: &str) -> PathBuf {
    let dir = temp_dir(label);
    let bin = env!("CARGO_BIN_EXE_legend");

    // `git init --initial-branch=main` instead of relying on the
    // user's default-branch config (which may be `master`). Older git
    // versions don't accept the flag — we set init.defaultBranch
    // first as a fallback.
    let _ = std::process::Command::new("git")
        .current_dir(&dir)
        .args(["init", "--initial-branch=main"])
        .output();
    // If `init --initial-branch` didn't actually create the repo (old
    // git), fall back to plain init + branch rename. Idempotent.
    if !dir.join(".git").exists() {
        run_git(&dir, &["init"]);
        // Tolerate "branch already exists" by ignoring exit code here.
        let _ = run_git_output(&dir, &["branch", "-M", "main"]);
    }

    // Per-repo user identity (commits require it).
    run_git(&dir, &["config", "user.email", "test@legend.dev"]);
    run_git(&dir, &["config", "user.name", "Legend Test"]);
    // Don't sign — this test runs in CI environments without keys.
    run_git(&dir, &["config", "commit.gpgsign", "false"]);

    // Register the merge driver. Same format the `legend init`
    // command writes — duplicating here keeps the test self-contained
    // (no dependency on having run `init` against this temp dir).
    let driver_cmd = format!("{bin} git-merge-driver %O %A %B %P");
    run_git(&dir, &["config", "merge.legend.driver", &driver_cmd]);

    // Tell git to route `.legend/memory.lz4` through that driver.
    let attrs = b".legend/memory.lz4 merge=legend\n";
    fs::write(dir.join(".gitattributes"), attrs).expect("write .gitattributes");
    fs::create_dir_all(dir.join(".legend")).expect("create .legend dir");

    dir
}

#[test]
fn real_git_merge_auto_resolves_memory_lz4_conflict() {
    let dir = init_repo_with_driver("real_git_merge");
    let snapshot = dir.join(".legend/memory.lz4");

    // Commit 1 (main): the seed substrate. Both branches will diverge
    // from this point.
    persistence::save(&load_seed_graph(), &snapshot).expect("save seed snapshot");
    run_git(&dir, &["add", ".gitattributes", ".legend/memory.lz4"]);
    run_git(&dir, &["commit", "-m", "seed substrate"]);

    // Branch `theirs`: add Beam to the substrate, commit.
    run_git(&dir, &["checkout", "-b", "theirs"]);
    let mut hypergraph_theirs = persistence::load(&snapshot).expect("load on theirs");
    tick(&mut hypergraph_theirs, "Beam is a Go server.");
    persistence::save(&hypergraph_theirs, &snapshot).expect("save theirs");
    run_git(&dir, &["commit", "-am", "theirs: Beam"]);

    // Back to main: add Polaris instead, commit.
    run_git(&dir, &["checkout", "main"]);
    let mut hypergraph_main = persistence::load(&snapshot).expect("load on main");
    tick(&mut hypergraph_main, "Polaris is written in Rust.");
    persistence::save(&hypergraph_main, &snapshot).expect("save main");
    run_git(&dir, &["commit", "-am", "main: Polaris"]);

    // Without the merge driver, this would CONFLICT on memory.lz4.
    // With it, git invokes `legend git-merge-driver` to substrate-
    // merge both sides. `--no-edit` skips the editor prompt for the
    // merge commit message.
    let output = run_git_output(&dir, &["merge", "--no-edit", "theirs"]);
    assert!(
        output.status.success(),
        "git merge failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    // git emits "Auto-merging .legend/memory.lz4" when a driver runs;
    // our driver echoes "[legend] merging memory.lz4" / "[legend]
    // merged: …" to stderr. Confirm at least one of those landed.
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stderr}{stdout}");
    assert!(
        combined.contains("[legend] merging") || combined.contains("[legend] merged:"),
        "expected legend merge driver output; got stderr=\n{stderr}\nstdout=\n{stdout}",
    );

    // The merged snapshot must carry BOTH branches' contributions.
    let merged = persistence::load(&snapshot).expect("load merged snapshot");
    assert!(
        merged.by_name.contains_key("Polaris"),
        "merged graph missing Polaris (from main)"
    );
    assert!(
        merged.by_name.contains_key("Beam"),
        "merged graph missing Beam (from theirs)"
    );

    // Working tree should be clean (no unresolved conflicts).
    let status = run_git_output(&dir, &["status", "--porcelain"]);
    let porcelain = String::from_utf8_lossy(&status.stdout);
    assert!(
        !porcelain.contains("UU "),
        "unresolved conflict markers in git status:\n{porcelain}",
    );

    // There should be a real merge commit (two parents).
    let parents = run_git_output(&dir, &["rev-list", "--parents", "-n", "1", "HEAD"]);
    let parent_line = String::from_utf8_lossy(&parents.stdout);
    let parent_count = parent_line.split_whitespace().count() - 1; // first is HEAD
    assert_eq!(
        parent_count, 2,
        "expected a merge commit with 2 parents; got {parent_count}",
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn real_git_merge_falls_back_to_conflict_when_driver_errors() {
    // If the driver exits non-zero (e.g. seed drift), git should
    // leave the file in a conflicted state — `git status` reports
    // it as UU, exit code is non-zero. This verifies the error path
    // doesn't silently corrupt the working tree.
    let dir = init_repo_with_driver("driver_error");
    let snapshot = dir.join(".legend/memory.lz4");

    persistence::save(&load_seed_graph(), &snapshot).unwrap();
    run_git(&dir, &["add", ".gitattributes", ".legend/memory.lz4"]);
    run_git(&dir, &["commit", "-m", "base"]);

    run_git(&dir, &["checkout", "-b", "drift"]);
    // Replace theirs's snapshot with a tampered-fingerprint blob —
    // the driver's persistence::load call will refuse it with
    // PersistError::SeedDrift.
    let mut tampered = Vec::new();
    tampered.extend_from_slice(b"LEGEND01");
    tampered.extend_from_slice(&1u32.to_le_bytes());
    tampered.extend_from_slice(&[0xAB; 8]);
    tampered.extend_from_slice(&[0u8; 32]);
    fs::write(&snapshot, &tampered).unwrap();
    run_git(&dir, &["commit", "-am", "drift: tampered"]);

    run_git(&dir, &["checkout", "main"]);
    let mut hypergraph_main = persistence::load(&snapshot).unwrap();
    tick(&mut hypergraph_main, "Polaris is a project.");
    persistence::save(&hypergraph_main, &snapshot).unwrap();
    run_git(&dir, &["commit", "-am", "main: Polaris"]);

    let output = run_git_output(&dir, &["merge", "--no-edit", "drift"]);
    // git reports non-zero when the driver fails — the merge is
    // incomplete and the user must resolve manually (or abort).
    assert!(
        !output.status.success(),
        "git merge should have failed when driver returned error",
    );
    // The driver's stderr should include the SeedDrift hint.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.to_lowercase().contains("seed") || stderr.to_lowercase().contains("drift"),
        "expected SeedDrift mention in stderr; got:\n{stderr}",
    );
    // Abort so we don't leave the temp repo in a weird state.
    let _ = run_git_output(&dir, &["merge", "--abort"]);

    let _ = fs::remove_dir_all(&dir);
}
