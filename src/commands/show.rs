// Show command - human-readable display of Legend state
//
// Unlike get_state (JSON for Claude), this is for YOU in the terminal
//
// Rust concepts in this file:
// - String formatting with format!() and padding
// - Sorting with sort_by() and closures
// - Iterator methods: map, filter, collect
// - Display trait basics (how Rust converts types to strings)

use crate::storage;
use crate::types::FeatureStatus;

/// Handle the show command
///
/// Loads state and prints a formatted table sorted by recency
pub fn handle_show() -> Result<(), Box<dyn std::error::Error>> {
    let mut state = storage::load_state()?;

    if state.features.is_empty() {
        println!("No features tracked yet. Use 'legend update' to add features.");
        return Ok(());
    }

    // Sort by recency score (highest first)
    // sort_by uses a closure that compares two features
    // partial_cmp handles f64 comparison (which can be NaN)
    // We reverse (b.cmp(a)) so highest recency comes first
    state.features.sort_by(|a, b| {
        b.recency_score
            .partial_cmp(&a.recency_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Print header
    println!(
        "{:<20} {:<14} {:<12} {:<8} {}",
        "ID", "DOMAIN", "STATUS", "RECENCY", "NAME"
    );
    println!("{}", "-".repeat(72));

    // Print each feature
    for feature in &state.features {
        let status_str = status_label(feature.status);
        let recency_str = format!("{:.0}%", feature.recency_score * 100.0);

        println!(
            "{:<20} {:<14} {:<12} {:<8} {}",
            truncate(&feature.id, 19),
            truncate(&feature.domain, 13),
            status_str,
            recency_str,
            feature.name,
        );
    }

    // Summary line
    println!("{}", "-".repeat(72));

    let complete = state
        .features
        .iter()
        .filter(|f| f.status == FeatureStatus::Complete)
        .count();
    let total = state.features.len();

    println!("{}/{} features complete", complete, total);

    Ok(())
}

/// Convert FeatureStatus to a display string
fn status_label(status: FeatureStatus) -> &'static str {
    match status {
        FeatureStatus::Pending => "Pending",
        FeatureStatus::InProgress => "InProgress",
        FeatureStatus::Blocked => "Blocked",
        FeatureStatus::Complete => "Complete",
    }
}

/// Truncate a string to max_len, adding ".." if truncated
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}..", &s[..max_len - 2])
    }
}
