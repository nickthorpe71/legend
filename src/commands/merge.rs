use crate::memory::{load_memory_from_path, save_memory_to_path};
use crate::storage::{load_state_from_path, save_state_to_path};

/// Git merge driver for Legend state files (.lz4).
///
/// Usage: legend git-merge-driver %O %A %B %P
/// %O: Ancestor's version (base)
/// %A: Current version (ours)
/// %B: Other version (theirs)
/// %P: The filename
pub fn handle_git_merge_driver(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.len() < 4 {
        return Err("Usage: legend git-merge-driver %O %A %B %P".into());
    }

    let _ancestor_path = &args[0];
    let ours_path = &args[1];
    let theirs_path = &args[2];
    let filename = &args[3];

    eprintln!("[LEGEND] Auto-merging conflicted state file: {}", filename);

    if filename.ends_with("state.lz4") {
        let mut ours = load_state_from_path(ours_path)?;
        let theirs = load_state_from_path(theirs_path)?;
        // We don't strictly need ancestor for our timestamp-based merge, but could use it for deletion detection if needed.
        
        ours.merge(theirs);
        save_state_to_path(&ours, ours_path)?;
    } else if filename.ends_with("memory.lz4") {
        let mut ours = load_memory_from_path(ours_path)?;
        let theirs = load_memory_from_path(theirs_path)?;
        
        ours.merge_from_log(theirs);
        save_memory_to_path(&ours, ours_path)?;
    } else {
        return Err(format!("Unknown file type for merge: {}", filename).into());
    }

    Ok(())
}
