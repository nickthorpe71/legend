use crate::storage;
use crate::types::FeatureStatus;

/// Display features as a human-readable table sorted by recency.
pub fn handle_show() -> Result<(), Box<dyn std::error::Error>> {
    let mut state = storage::load_state()?;

    if state.features.is_empty() {
        println!("No features tracked yet. Use 'legend update' to add features.");
        return Ok(());
    }

    state.features.sort_by(|a, b| {
        b.recency_score
            .partial_cmp(&a.recency_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    println!(
        "{:<20} {:<14} {:<12} {:<8} NAME",
        "ID", "DOMAIN", "STATUS", "RECENCY"
    );
    println!("{}", "-".repeat(72));

    for feature in &state.features {
        println!(
            "{:<20} {:<14} {:<12} {:<8} {}",
            truncate(&feature.id, 19),
            truncate(&feature.domain, 13),
            status_label(feature.status),
            format!("{:.0}%", feature.recency_score * 100.0),
            feature.name,
        );
    }

    println!("{}", "-".repeat(72));
    let complete = state
        .features
        .iter()
        .filter(|f| f.status == FeatureStatus::Complete)
        .count();
    println!("{}/{} features complete", complete, state.features.len());

    Ok(())
}

fn status_label(status: FeatureStatus) -> &'static str {
    match status {
        FeatureStatus::Pending => "Pending",
        FeatureStatus::InProgress => "InProgress",
        FeatureStatus::Blocked => "Blocked",
        FeatureStatus::Complete => "Complete",
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}..", &s[..max_len - 2])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_label() {
        assert_eq!(status_label(FeatureStatus::Pending), "Pending");
        assert_eq!(status_label(FeatureStatus::InProgress), "InProgress");
        assert_eq!(status_label(FeatureStatus::Blocked), "Blocked");
        assert_eq!(status_label(FeatureStatus::Complete), "Complete");
    }

    #[test]
    fn test_truncate_short() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_exact() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_long() {
        assert_eq!(truncate("hello world", 7), "hello..");
    }
}
