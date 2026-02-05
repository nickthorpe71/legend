// Update command - merges incoming changes from Claude into state
//
// This is the WRITE PATH (100-500ms acceptable)
// Called after Claude finishes responding, so latency is hidden
//
// Rust concepts in this file:
// - std::io::stdin() for reading input
// - HashMap for O(1) lookups during merge
// - Mutable references (&mut) for in-place updates
// - Iterators and closures for data transformation
// - Time handling for recency scores

use crate::storage::{load_state, save_state};
use crate::types::{Feature, FeatureStatus, LegendState};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::{self, Read};
use std::time::{SystemTime, UNIX_EPOCH};

// Update struct - what Claude sends us via stdin
//
// This mirrors the structure Claude outputs when tracking features
// Serde handles JSON -> Rust struct conversion automatically
#[derive(Debug, Deserialize)]
pub struct Update {
    // Features to add or update
    // If a feature ID exists, we update it; otherwise, we add it
    #[serde(default)]
    pub features: Vec<FeatureUpdate>,

    // Optional: features to remove by ID
    #[serde(default)]
    pub remove_features: Vec<String>,
}

// FeatureUpdate - a single feature being added or updated
//
// Why separate from Feature? Claude shouldn't need to provide
// every field - we'll use defaults and preserve existing values
#[derive(Debug, Deserialize)]
pub struct FeatureUpdate {
    pub id: String,
    pub name: Option<String>,
    pub domain: Option<String>,
    pub description: Option<String>,
    pub status: Option<FeatureStatus>,
    pub tags: Option<Vec<String>>,
    pub context: Option<String>,
    pub files_involved: Option<Vec<String>>,
}

/// Handle the update command
///
/// Flow:
/// 1. Read JSON from stdin
/// 2. Parse into Update struct
/// 3. Load existing state
/// 4. Merge updates into state
/// 5. Recalculate recency scores
/// 6. Save state back to disk
pub fn handle_update() -> Result<(), Box<dyn std::error::Error>> {
    // Step 1: Read JSON from stdin
    // This allows piping: echo '{"features": [...]}' | legend update
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| format!("Failed to read stdin: {}", e))?;

    // Handle empty input gracefully
    if input.trim().is_empty() {
        return Err("No input provided. Pipe JSON to stdin.".into());
    }

    // Step 2: Parse JSON into Update struct
    // serde_json::from_str automatically deserializes based on the type
    let update: Update = serde_json::from_str(&input)
        .map_err(|e| format!("Failed to parse JSON: {}", e))?;

    // Step 3: Load existing state
    let mut state = load_state()?;

    // Step 4: Merge updates into state
    merge_updates(&mut state, update)?;

    // Step 5: Recalculate recency scores for all features
    recalculate_recency_scores(&mut state);

    // Step 6: Save state back to disk
    save_state(&state)?;

    // Report what we did
    println!(
        "Updated state: {} features total",
        state.features.len()
    );

    Ok(())
}

/// Merge incoming updates into existing state
///
/// Strategy:
/// - Build a HashMap of existing features for O(1) lookup
/// - For each incoming feature:
///   - If exists: update only the provided fields
///   - If new: create with required fields, defaults for rest
/// - Remove any features in the remove list
fn merge_updates(
    state: &mut LegendState,
    update: Update,
) -> Result<(), Box<dyn std::error::Error>> {
    let now = current_timestamp();

    // Build a HashMap for fast lookups by ID
    // Why HashMap? O(1) lookup vs O(n) linear search
    // We're mapping feature ID -> index in the features Vec
    let mut id_to_index: HashMap<String, usize> = state
        .features
        .iter()
        .enumerate()
        .map(|(i, f)| (f.id.clone(), i))
        .collect();

    // Process each feature update
    for feature_update in update.features {
        if let Some(&index) = id_to_index.get(&feature_update.id) {
            // Feature exists - update it in place
            let existing = &mut state.features[index];
            apply_update(existing, feature_update, now);
        } else {
            // New feature - create it
            let new_feature = create_feature_from_update(feature_update, now)?;
            let new_index = state.features.len();
            id_to_index.insert(new_feature.id.clone(), new_index);
            state.features.push(new_feature);
        }
    }

    // Remove features marked for deletion
    // We need to filter, not iterate and remove (borrow checker!)
    if !update.remove_features.is_empty() {
        // Create a set for O(1) removal checks
        let remove_set: std::collections::HashSet<_> =
            update.remove_features.into_iter().collect();

        // retain() keeps elements where the closure returns true
        state.features.retain(|f| !remove_set.contains(&f.id));
    }

    // Update state's last_updated timestamp
    state.touch();

    Ok(())
}

/// Apply an update to an existing feature
///
/// Only updates fields that are Some (provided)
/// Preserves existing values for None fields
fn apply_update(feature: &mut Feature, update: FeatureUpdate, now: i64) {
    // Update only provided fields using if-let pattern
    // This is idiomatic Rust for "update if present"

    if let Some(name) = update.name {
        feature.name = name;
    }

    if let Some(domain) = update.domain {
        feature.domain = domain;
    }

    if let Some(description) = update.description {
        feature.description = description;
    }

    if let Some(status) = update.status {
        feature.status = status;
    }

    if let Some(tags) = update.tags {
        feature.tags = tags;
    }

    // context is Option<String>, so we handle it specially
    // If update provides Some(context), we set it
    // If update provides None, we leave existing value
    if update.context.is_some() {
        feature.context = update.context;
    }

    if let Some(files) = update.files_involved {
        feature.files_involved = files;
    }

    // Always update the timestamp when touched
    feature.last_updated = now;
}

/// Create a new Feature from an update
///
/// Requires at minimum: id, name, domain, description
/// Returns error if required fields are missing
fn create_feature_from_update(
    update: FeatureUpdate,
    now: i64,
) -> Result<Feature, Box<dyn std::error::Error>> {
    // For new features, we need certain fields
    let name = update.name.ok_or_else(|| {
        format!("New feature '{}' requires 'name' field", update.id)
    })?;

    let domain = update.domain.ok_or_else(|| {
        format!("New feature '{}' requires 'domain' field", update.id)
    })?;

    let description = update.description.ok_or_else(|| {
        format!("New feature '{}' requires 'description' field", update.id)
    })?;

    Ok(Feature {
        id: update.id,
        name,
        domain,
        description,
        status: update.status.unwrap_or(FeatureStatus::Pending),
        tags: update.tags.unwrap_or_default(),
        context: update.context,
        files_involved: update.files_involved.unwrap_or_default(),
        created_at: now,
        last_updated: now,
        recency_score: 1.0, // New features start at max recency
    })
}

/// Recalculate recency scores for all features
///
/// Algorithm: Exponential decay based on time since last update
/// - Most recent feature gets score 1.0
/// - Score decays by half every 7 days (configurable)
///
/// Why exponential decay?
/// - Recent work is more relevant than old work
/// - Smooth curve (no sudden drops)
/// - Easy to tune with half-life parameter
fn recalculate_recency_scores(state: &mut LegendState) {
    let now = current_timestamp();

    // Half-life in seconds (7 days)
    // After 7 days, a feature's recency score is halved
    const HALF_LIFE_SECONDS: f64 = 7.0 * 24.0 * 60.0 * 60.0;

    // Natural log of 2 (for decay formula)
    const LN_2: f64 = 0.693147;

    for feature in &mut state.features {
        // Time since last update in seconds
        let age_seconds = (now - feature.last_updated) as f64;

        // Exponential decay formula: score = e^(-λt)
        // where λ = ln(2) / half_life
        let decay_rate = LN_2 / HALF_LIFE_SECONDS;
        let score = (-decay_rate * age_seconds).exp();

        // Clamp to reasonable range [0.01, 1.0]
        // Never go to 0 - old features still have some relevance
        feature.recency_score = score.clamp(0.01, 1.0);
    }
}

/// Get current Unix timestamp
fn current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_update() {
        let json = r#"{
            "features": [
                {
                    "id": "auth",
                    "name": "Authentication",
                    "domain": "security",
                    "description": "User login system"
                }
            ]
        }"#;

        let update: Update = serde_json::from_str(json).unwrap();
        assert_eq!(update.features.len(), 1);
        assert_eq!(update.features[0].id, "auth");
    }

    #[test]
    fn test_parse_partial_update() {
        let json = r#"{
            "features": [
                {
                    "id": "auth",
                    "status": "Complete"
                }
            ]
        }"#;

        let update: Update = serde_json::from_str(json).unwrap();
        assert_eq!(update.features.len(), 1);
        assert_eq!(update.features[0].id, "auth");
        assert!(update.features[0].name.is_none());
        assert_eq!(update.features[0].status, Some(FeatureStatus::Complete));
    }

    #[test]
    fn test_recency_decay() {
        let mut state = LegendState::new("Test".to_string());

        // Add a feature with an old timestamp (30 days ago)
        let mut old_feature = Feature::new(
            "old".to_string(),
            "Old Feature".to_string(),
            "test".to_string(),
            "An old feature".to_string(),
        );
        old_feature.last_updated = current_timestamp() - (30 * 24 * 60 * 60);
        state.add_feature(old_feature);

        // Add a recent feature
        let new_feature = Feature::new(
            "new".to_string(),
            "New Feature".to_string(),
            "test".to_string(),
            "A new feature".to_string(),
        );
        state.add_feature(new_feature);

        // Recalculate scores
        recalculate_recency_scores(&mut state);

        // New feature should have higher recency score
        let old_score = state.find_feature("old").unwrap().recency_score;
        let new_score = state.find_feature("new").unwrap().recency_score;

        assert!(new_score > old_score, "New features should have higher recency");
        assert!(new_score > 0.9, "Recent feature should be close to 1.0");
        assert!(old_score < 0.1, "30-day-old feature should have low recency");
    }
}
