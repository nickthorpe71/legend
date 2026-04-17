use crate::memory::{load_memory_from_path, save_memory_to_path};

/// Git merge driver for Legend memory files (memory.lz4 and events.jsonl).
///
/// Usage: legend git-merge-driver %O %A %B %P
/// %O: Ancestor's version (base)
/// %A: Current version (ours)
/// %B: Other version (theirs)
/// %P: The filename
pub fn handle_git_merge_driver(
    args: &[String],
    _def: &crate::cli::CommandDef,
) -> Result<(), Box<dyn std::error::Error>> {
    if args.len() < 4 {
        return Err("Usage: legend git-merge-driver %O %A %B %P".into());
    }

    let ancestor_path = &args[0];
    let ours_path = &args[1];
    let theirs_path = &args[2];
    let filename = &args[3];

    eprintln!("[LEGEND] Auto-merging conflicted state file: {}", filename);

    if filename.ends_with("memory.lz4") {
        let mut ours = load_memory_from_path(ours_path)?;
        let theirs = load_memory_from_path(theirs_path)?;

        let stats = crate::tool::merge_states(&mut ours, &theirs);
        save_memory_to_path(&ours, ours_path)?;
        eprintln!(
            "[LEGEND] Structural merge complete: {} entries merged",
            stats.total()
        );
    } else if filename.ends_with("events.jsonl") {
        merge_jsonl_events(ancestor_path, ours_path, theirs_path)?;
    } else {
        return Err(format!("Unsupported file type for merge: {}", filename).into());
    }

    Ok(())
}

/// Merge a pre-merge memory.lz4 file into the current state.
///
/// Used by the post-merge git hook to recover state that would otherwise
/// be lost during fast-forward pulls.
///
/// Usage: legend merge-local <path-to-pre-merge-memory.lz4>
pub fn handle_merge_local(
    args: &[String],
    _def: &crate::cli::CommandDef,
) -> Result<(), Box<dyn std::error::Error>> {
    if args.is_empty() {
        return Err("Usage: legend merge-local <path-to-pre-merge-memory.lz4>".into());
    }

    let pre_merge_path = &args[0];

    // Load the pre-merge state from the provided file
    let pre_merge = match load_memory_from_path(pre_merge_path) {
        Ok(state) => state,
        Err(e) => {
            eprintln!("[LEGEND] Could not load pre-merge state: {}", e);
            return Ok(()); // Non-fatal: hook should not break git pull
        }
    };

    // Load current (post-pull) state
    let mut current = match crate::memory::load_or_default() {
        Ok(state) => state,
        Err(e) => {
            eprintln!("[LEGEND] Could not load current state: {}", e);
            return Ok(());
        }
    };

    let stats = crate::tool::merge_states(&mut current, &pre_merge);

    if stats.total() > 0 {
        if let Err(e) = crate::memory::save(&current) {
            eprintln!("[LEGEND] Failed to save merged state: {}", e);
            return Ok(());
        }
        eprintln!(
            "[LEGEND] Post-merge recovery: merged {} entries from pre-pull state",
            stats.total()
        );
    }

    Ok(())
}

/// Merge an append-only JSONL event log.
///
/// Takes the common ancestor as a base, then appends any lines from ours and
/// theirs that are not in the ancestor, deduplicated and sorted by timestamp.
fn merge_jsonl_events(
    ancestor_path: &str,
    ours_path: &str,
    theirs_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::collections::{BTreeMap, HashSet};
    use std::fs;

    let ancestor = fs::read_to_string(ancestor_path).unwrap_or_default();
    let ours = fs::read_to_string(ours_path)?;
    let theirs = fs::read_to_string(theirs_path)?;

    let ancestor_lines: HashSet<&str> = ancestor.lines().filter(|l| !l.is_empty()).collect();

    // Collect new lines from each side, keyed by (timestamp, content) to
    // deduplicate identical events and sort chronologically.
    let mut new_lines: BTreeMap<(i64, String), ()> = BTreeMap::new();

    for line in ours.lines().chain(theirs.lines()) {
        if line.is_empty() || ancestor_lines.contains(line) {
            continue;
        }
        let ts = extract_ts(line);
        new_lines.entry((ts, line.to_string())).or_insert(());
    }

    // Result = ancestor lines (preserved in order) + new lines sorted by ts
    let mut result = String::new();
    for line in ancestor.lines() {
        if !line.is_empty() {
            result.push_str(line);
            result.push('\n');
        }
    }
    for (_, line) in new_lines.keys() {
        result.push_str(line);
        result.push('\n');
    }

    fs::write(ours_path, result)?;
    eprintln!(
        "[LEGEND] Merged events.jsonl: {} base + {} new lines",
        ancestor_lines.len(),
        new_lines.len()
    );
    Ok(())
}

/// Extract the "ts" field from a JSON line for sorting. Returns 0 if not found.
fn extract_ts(line: &str) -> i64 {
    if let Some(pos) = line.rfind("\"ts\":") {
        let rest = &line[pos + 5..];
        let num_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        num_str.parse().unwrap_or(0)
    } else {
        0
    }
}
