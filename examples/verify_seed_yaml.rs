//! Verify `seed_pack.yaml` survived the 14-region + 20-examples-per-region
//! migration. Checks shape, string round-trip integrity, and seeded-
//! relation consistency. Does NOT verify embeddings or runtime
//! behavior — only the YAML's own structural sanity.
//!
//! Run: `cargo run --release --example verify_seed_yaml`
//!
//! Throwaway — toss before prod.

#[path = "shared/mod.rs"]
mod shared;

use shared::{RawRelGroup, load_seed_pack};
use std::collections::HashSet;
use std::path::Path;

const EXPECTED_REGIONS: &[&str] = &[
    "REGION_ENTITIES",
    "REGION_EVENTS",
    "REGION_CHANGE_HISTORY",
    "REGION_RELATIONSHIPS",
    "REGION_QUANTITIES",
    "REGION_TIME",
    "REGION_LOCATIONS",
    "REGION_TASKS",
    "REGION_DECISIONS",
    "REGION_PREFERENCES",
    "REGION_DEFINITIONS",
    "REGION_PROVENANCE",
    "REGION_DOMAINS",
    "REGION_MODAL_NEGATED",
];

const EXAMPLES_PER_REGION: usize = 20;

/// Exact strings expected at known indices. Catches silent corruption
/// of escaped quotes, colons, apostrophes, URLs — the YAML-tricky bits.
const SPOT_CHECKS: &[(&str, usize, &str)] = &[
    ("REGION_RELATIONSHIPS", 0, "Sarah is my sister"),
    (
        "REGION_RELATIONSHIPS",
        19,
        "Sarah and Marcus collaborated on the paper",
    ),
    (
        "REGION_DEFINITIONS",
        16,
        "by \"consensus\" we mean unanimous agreement among participants",
    ),
    ("REGION_DOMAINS", 0, "in dental:"),
    ("REGION_DOMAINS", 2, "context: medical billing"),
    (
        "REGION_PROVENANCE",
        0,
        "see https://arxiv.org/abs/2310.04567 for the details",
    ),
    ("REGION_PROVENANCE", 19, "tracked in JIRA-1234"),
    (
        "REGION_MODAL_NEGATED",
        19,
        "we shouldn't proceed without legal sign-off",
    ),
    ("REGION_ENTITIES", 0, "this is Sarah, our lead designer"),
    ("REGION_PREFERENCES", 17, "I don't drink coffee after 2pm"),
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pack = load_seed_pack(Path::new("seed_pack.yaml"))?;
    let mut errors: Vec<String> = Vec::new();
    let mut info: Vec<String> = Vec::new();

    // 1. Region count
    if pack.regions.len() == 14 {
        info.push("✓ 14 regions present".into());
    } else {
        errors.push(format!(
            "region count: got {}, expected 14",
            pack.regions.len()
        ));
    }

    // 2. REGION_STATES absent
    if pack.regions.iter().any(|r| r.element_id == "REGION_STATES") {
        errors.push("REGION_STATES still present in regions list".into());
    } else {
        info.push("✓ REGION_STATES absent from regions list".into());
    }

    // 3. All 14 expected region IDs present
    let actual_ids: HashSet<&str> = pack.regions.iter().map(|r| r.element_id.as_str()).collect();
    let mut all_present = true;
    for &expected in EXPECTED_REGIONS {
        if !actual_ids.contains(expected) {
            errors.push(format!("missing expected region: {expected}"));
            all_present = false;
        }
    }
    if all_present {
        info.push("✓ All 14 expected region element_ids present".into());
    }

    // 4. Each region has exactly 20 examples
    let mut counts_ok = true;
    for region in &pack.regions {
        if region.examples.len() != EXAMPLES_PER_REGION {
            errors.push(format!(
                "{}: {} examples, expected {EXAMPLES_PER_REGION}",
                region.element_id,
                region.examples.len()
            ));
            counts_ok = false;
        }
    }
    if counts_ok {
        info.push(format!(
            "✓ Every region has exactly {EXAMPLES_PER_REGION} examples"
        ));
    }

    // 5. No empty or whitespace-only example strings
    let mut empty_ok = true;
    for region in &pack.regions {
        for (i, ex) in region.examples.iter().enumerate() {
            if ex.trim().is_empty() {
                errors.push(format!(
                    "{} example[{i}] is empty or whitespace-only",
                    region.element_id
                ));
                empty_ok = false;
            }
        }
    }
    if empty_ok {
        info.push("✓ No empty/whitespace-only example strings".into());
    }

    // 6. Spot-check critical strings round-tripped exactly
    let mut spot_ok = true;
    for &(rid, idx, expected) in SPOT_CHECKS {
        let region = match pack.regions.iter().find(|r| r.element_id == rid) {
            Some(r) => r,
            None => {
                errors.push(format!("spot check: region {rid} not found"));
                spot_ok = false;
                continue;
            }
        };
        match region.examples.get(idx) {
            Some(actual) if actual == expected => {}
            Some(actual) => {
                errors.push(format!(
                    "spot check {rid}[{idx}]:\n    got      {actual:?}\n    expected {expected:?}",
                ));
                spot_ok = false;
            }
            None => {
                errors.push(format!("spot check {rid}[{idx}]: index out of bounds"));
                spot_ok = false;
            }
        }
    }
    if spot_ok {
        info.push(format!(
            "✓ {} spot-check strings round-tripped exactly (escaped quotes, URLs, colons, apostrophes)",
            SPOT_CHECKS.len()
        ));
    }

    // 7. No within-region duplicate examples
    let mut dedup_ok = true;
    for region in &pack.regions {
        let unique: HashSet<&String> = region.examples.iter().collect();
        if unique.len() != region.examples.len() {
            errors.push(format!(
                "{}: has duplicate examples ({} unique / {} total)",
                region.element_id,
                unique.len(),
                region.examples.len()
            ));
            dedup_ok = false;
        }
    }
    if dedup_ok {
        info.push("✓ No within-region duplicate examples".into());
    }

    // 8. Cross-region duplicates (informational only — same sentence
    //    legitimately fitting two regions is a routing-ambiguity
    //    signal worth surfacing but not a Step 1 failure)
    let total: usize = pack.regions.iter().map(|r| r.examples.len()).sum();
    info.push(format!(
        "✓ {total} total examples across {} regions",
        pack.regions.len()
    ));
    let mut all_pairs: Vec<(&str, &str)> = Vec::with_capacity(total);
    for region in &pack.regions {
        for ex in &region.examples {
            all_pairs.push((region.element_id.as_str(), ex.as_str()));
        }
    }
    all_pairs.sort_by_key(|(_, ex)| *ex);
    let mut cross_dups: Vec<(&str, &str, &str)> = Vec::new();
    for window in all_pairs.windows(2) {
        if window[0].1 == window[1].1 && window[0].0 != window[1].0 {
            cross_dups.push((window[0].0, window[1].0, window[0].1));
        }
    }
    if cross_dups.is_empty() {
        info.push("  (no cross-region duplicate examples)".into());
    } else {
        info.push(format!(
            "  (informational: {} cross-region duplicate(s) — same string in multiple regions)",
            cross_dups.len()
        ));
        for (a, b, ex) in &cross_dups {
            info.push(format!("    {a} ↔ {b}: {ex:?}"));
        }
    }

    // 9. Seeded relations don't reference REGION_STATES
    let mut orphan_states = Vec::new();
    scan_group_for_states(
        "region_class_pins",
        &pack.seeded_relations.region_class_pins,
        &mut orphan_states,
    );
    scan_group_for_states(
        "region_parent_pins",
        &pack.seeded_relations.region_parent_pins,
        &mut orphan_states,
    );
    scan_group_for_states(
        "reference_frame_class_pins",
        &pack.seeded_relations.reference_frame_class_pins,
        &mut orphan_states,
    );
    if orphan_states.is_empty() {
        info.push("✓ No seeded relations reference REGION_STATES".into());
    } else {
        for s in &orphan_states {
            errors.push(format!("orphan REGION_STATES reference: {s}"));
        }
    }

    // 10. Pin counts match region count
    let class_pins = pack.seeded_relations.region_class_pins.relations.len();
    let parent_pins = pack.seeded_relations.region_parent_pins.relations.len();
    if class_pins == 14 {
        info.push("✓ region_class_pins has 14 entries (matches region count)".into());
    } else {
        errors.push(format!(
            "region_class_pins: {class_pins} entries, expected 14"
        ));
    }
    if parent_pins == 14 {
        info.push("✓ region_parent_pins has 14 entries (matches region count)".into());
    } else {
        errors.push(format!(
            "region_parent_pins: {parent_pins} entries, expected 14"
        ));
    }

    // Per-region summary (informational)
    info.push("".into());
    info.push("Per-region example counts:".into());
    for region in &pack.regions {
        info.push(format!(
            "  {:<25} {:>3} examples   (first: {:?})",
            region.element_id,
            region.examples.len(),
            region.examples.first().unwrap_or(&"<NONE>".into()),
        ));
    }

    // Output
    for line in &info {
        println!("{line}");
    }
    println!();
    if errors.is_empty() {
        println!(
            "=== Step 1 verification: PASS ({} checks) ===",
            info.iter().filter(|l| l.starts_with("✓")).count()
        );
        Ok(())
    } else {
        println!(
            "=== Step 1 verification: FAIL ({} error(s)) ===",
            errors.len()
        );
        for err in &errors {
            println!("  ✗ {err}");
        }
        std::process::exit(1);
    }
}

fn scan_group_for_states(group_name: &str, group: &RawRelGroup, found: &mut Vec<String>) {
    for (i, entry) in group.relations.iter().enumerate() {
        for value in entry {
            if let serde_yaml::Value::String(s) = value
                && s == "REGION_STATES"
            {
                found.push(format!("{group_name}[{i}]: {entry:?}"));
                break;
            }
        }
    }
}
