use crate::storage::{load_state, save_state};
use crate::types::{current_timestamp, Feature, FeatureStatus, LegendState};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::{self, Read};

#[derive(Debug, Deserialize)]
pub struct Update {
    #[serde(default)]
    pub features: Vec<FeatureUpdate>,
    #[serde(default)]
    pub remove_features: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
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

/// Read JSON from stdin, merge updates into state, recalculate recency, and save.
pub fn handle_update() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| format!("Failed to read stdin: {}", e))?;

    if input.trim().is_empty() {
        return Err("No input provided. Pipe JSON to stdin.".into());
    }

    let update: Update =
        serde_json::from_str(&input).map_err(|e| format!("Failed to parse JSON: {}", e))?;

    let mut state = load_state()?;
    merge_updates(&mut state, update)?;
    recalculate_recency_scores(&mut state);
    save_state(&state)?;

    println!("Updated state: {} features total", state.features.len());
    Ok(())
}

fn merge_updates(
    state: &mut LegendState,
    update: Update,
) -> Result<(), Box<dyn std::error::Error>> {
    let now = current_timestamp();

    let mut id_to_index: HashMap<String, usize> = state
        .features
        .iter()
        .enumerate()
        .map(|(i, f)| (f.id.clone(), i))
        .collect();

    for feature_update in update.features {
        if let Some(&index) = id_to_index.get(&feature_update.id) {
            apply_update(&mut state.features[index], feature_update, now);
        } else {
            let new_feature = create_feature_from_update(feature_update, now)?;
            let new_index = state.features.len();
            id_to_index.insert(new_feature.id.clone(), new_index);
            state.features.push(new_feature);
        }
    }

    if !update.remove_features.is_empty() {
        let remove_set: std::collections::HashSet<_> = update.remove_features.into_iter().collect();
        state.features.retain(|f| !remove_set.contains(&f.id));
    }

    state.touch();
    Ok(())
}

/// Apply partial update to an existing feature (only Some fields are overwritten).
fn apply_update(feature: &mut Feature, update: FeatureUpdate, now: i64) {
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
    if update.context.is_some() {
        feature.context = update.context;
    }
    if let Some(files) = update.files_involved {
        feature.files_involved = files;
    }
    feature.last_updated = now;
}

fn create_feature_from_update(
    update: FeatureUpdate,
    now: i64,
) -> Result<Feature, Box<dyn std::error::Error>> {
    let name = update
        .name
        .ok_or_else(|| format!("New feature '{}' requires 'name' field", update.id))?;
    let domain = update
        .domain
        .ok_or_else(|| format!("New feature '{}' requires 'domain' field", update.id))?;
    let description = update
        .description
        .ok_or_else(|| format!("New feature '{}' requires 'description' field", update.id))?;

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
        recency_score: 1.0,
    })
}

/// Exponential decay based on time since last update (half-life = 7 days).
fn recalculate_recency_scores(state: &mut LegendState) {
    let now = current_timestamp();
    const HALF_LIFE_SECS: f64 = 7.0 * 24.0 * 60.0 * 60.0;
    let decay_rate = std::f64::consts::LN_2 / HALF_LIFE_SECS;

    for feature in &mut state.features {
        let age = (now - feature.last_updated) as f64;
        feature.recency_score = (-decay_rate * age).exp().clamp(0.01, 1.0);
    }
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

        assert!(
            new_score > old_score,
            "New features should have higher recency"
        );
        assert!(new_score > 0.9, "Recent feature should be close to 1.0");
        assert!(
            old_score < 0.1,
            "30-day-old feature should have low recency"
        );
    }

    #[test]
    fn test_recency_clamp_minimum() {
        let mut state = LegendState::new("Test".to_string());
        let mut ancient = Feature::new(
            "ancient".to_string(),
            "Ancient".to_string(),
            "test".to_string(),
            "Very old".to_string(),
        );
        ancient.last_updated = current_timestamp() - (365 * 24 * 60 * 60); // 1 year ago
        state.add_feature(ancient);
        recalculate_recency_scores(&mut state);
        let score = state.find_feature("ancient").unwrap().recency_score;
        assert!(score >= 0.01, "Score should be clamped to 0.01 minimum, got {}", score);
    }

    #[test]
    fn test_apply_update_partial() {
        let now = current_timestamp();
        let mut feature = Feature::new(
            "test".to_string(),
            "Original".to_string(),
            "core".to_string(),
            "Original desc".to_string(),
        );
        let update = FeatureUpdate {
            id: "test".to_string(),
            name: Some("Updated Name".to_string()),
            domain: None,
            description: None,
            status: Some(FeatureStatus::Complete),
            tags: None,
            context: None,
            files_involved: None,
        };
        apply_update(&mut feature, update, now);
        assert_eq!(feature.name, "Updated Name");
        assert_eq!(feature.domain, "core"); // unchanged
        assert_eq!(feature.description, "Original desc"); // unchanged
        assert_eq!(feature.status, FeatureStatus::Complete);
        assert_eq!(feature.last_updated, now);
    }

    #[test]
    fn test_apply_update_context_set() {
        let now = current_timestamp();
        let mut feature = Feature::new(
            "test".to_string(),
            "Test".to_string(),
            "core".to_string(),
            "Desc".to_string(),
        );
        let update = FeatureUpdate {
            id: "test".to_string(),
            name: None,
            domain: None,
            description: None,
            status: None,
            tags: None,
            context: Some("New context".to_string()),
            files_involved: None,
        };
        apply_update(&mut feature, update, now);
        assert_eq!(feature.context, Some("New context".to_string()));
    }

    #[test]
    fn test_create_feature_from_update_requires_fields() {
        let now = current_timestamp();
        // Missing required fields
        let update = FeatureUpdate {
            id: "test".to_string(),
            name: None,
            domain: None,
            description: None,
            status: None,
            tags: None,
            context: None,
            files_involved: None,
        };
        assert!(create_feature_from_update(update, now).is_err());
    }

    #[test]
    fn test_create_feature_from_update_success() {
        let now = current_timestamp();
        let update = FeatureUpdate {
            id: "new".to_string(),
            name: Some("New Feature".to_string()),
            domain: Some("core".to_string()),
            description: Some("A brand new feature".to_string()),
            status: None, // defaults to Pending
            tags: Some(vec!["tag1".to_string()]),
            context: Some("Some context".to_string()),
            files_involved: Some(vec!["src/main.rs".to_string()]),
        };
        let feature = create_feature_from_update(update, now).unwrap();
        assert_eq!(feature.id, "new");
        assert_eq!(feature.name, "New Feature");
        assert_eq!(feature.status, FeatureStatus::Pending);
        assert_eq!(feature.recency_score, 1.0);
        assert_eq!(feature.created_at, now);
    }

    #[test]
    fn test_merge_updates_add_and_modify() {
        let mut state = LegendState::new("Test".to_string());
        state.add_feature(Feature::new(
            "existing".to_string(),
            "Existing".to_string(),
            "core".to_string(),
            "Existing desc".to_string(),
        ));
        let update = Update {
            features: vec![
                FeatureUpdate {
                    id: "existing".to_string(),
                    name: Some("Updated".to_string()),
                    domain: None,
                    description: None,
                    status: None,
                    tags: None,
                    context: None,
                    files_involved: None,
                },
                FeatureUpdate {
                    id: "new_one".to_string(),
                    name: Some("New".to_string()),
                    domain: Some("test".to_string()),
                    description: Some("desc".to_string()),
                    status: None,
                    tags: None,
                    context: None,
                    files_involved: None,
                },
            ],
            remove_features: vec![],
        };
        merge_updates(&mut state, update).unwrap();
        assert_eq!(state.features.len(), 2);
        assert_eq!(state.find_feature("existing").unwrap().name, "Updated");
        assert!(state.find_feature("new_one").is_some());
    }

    #[test]
    fn test_merge_updates_remove() {
        let mut state = LegendState::new("Test".to_string());
        state.add_feature(Feature::new(
            "to_remove".to_string(),
            "Remove Me".to_string(),
            "core".to_string(),
            "desc".to_string(),
        ));
        state.add_feature(Feature::new(
            "keep".to_string(),
            "Keep Me".to_string(),
            "core".to_string(),
            "desc".to_string(),
        ));
        let update = Update {
            features: vec![],
            remove_features: vec!["to_remove".to_string()],
        };
        merge_updates(&mut state, update).unwrap();
        assert_eq!(state.features.len(), 1);
        assert!(state.find_feature("to_remove").is_none());
        assert!(state.find_feature("keep").is_some());
    }
}
