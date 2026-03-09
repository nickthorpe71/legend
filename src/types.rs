use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FeatureStatus {
    #[default]
    Pending,
    InProgress,
    Blocked,
    Complete,
}

/// A single tracked feature.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Feature {
    pub id: String,
    pub name: String,
    pub domain: String,
    pub tags: Vec<String>,
    pub status: FeatureStatus,
    pub description: String,
    pub context: Option<String>,
    pub files_involved: Vec<String>,
    pub created_at: i64,
    pub last_updated: i64,
    pub recency_score: f64,
}

#[allow(dead_code)]
impl Feature {
    pub fn new(id: String, name: String, domain: String, description: String) -> Self {
        let now = current_timestamp();
        Feature {
            id,
            name,
            domain,
            description,
            status: FeatureStatus::Pending,
            tags: Vec::new(),
            context: None,
            files_involved: Vec::new(),
            created_at: now,
            last_updated: now,
            recency_score: 1.0,
        }
    }

    pub fn is_complete(&self) -> bool {
        self.status == FeatureStatus::Complete
    }

    pub fn touch(&mut self) {
        self.last_updated = current_timestamp();
    }

    pub fn mark_complete(&mut self) {
        self.status = FeatureStatus::Complete;
        self.touch();
    }
}

/// The entire persisted state for a Legend project.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct LegendState {
    pub project_name: String,
    pub features: Vec<Feature>,
    pub created_at: i64,
    pub last_updated: i64,
}

#[allow(dead_code)]
impl LegendState {
    pub fn new(project_name: String) -> Self {
        let now = current_timestamp();
        LegendState {
            project_name,
            features: Vec::new(),
            created_at: now,
            last_updated: now,
        }
    }

    pub fn add_feature(&mut self, feature: Feature) {
        self.features.push(feature);
        self.touch();
    }

    pub fn find_feature(&self, id: &str) -> Option<&Feature> {
        self.features.iter().find(|f| f.id == id)
    }

    /// Merge another LegendState into this one.
    /// Features with the same ID are merged by taking the one with the latest last_updated timestamp.
    pub fn merge(&mut self, other: LegendState) {
        let mut feature_map: HashMap<String, Feature> = self
            .features
            .drain(..)
            .map(|f| (f.id.clone(), f))
            .collect();

        for other_feature in other.features {
            if let Some(existing) = feature_map.get_mut(&other_feature.id) {
                if other_feature.last_updated > existing.last_updated {
                    *existing = other_feature;
                }
            } else {
                feature_map.insert(other_feature.id.clone(), other_feature);
            }
        }

        self.features = feature_map.into_values().collect();
        if self.project_name.is_empty() {
            self.project_name = other.project_name;
        }
        self.created_at = self.created_at.min(other.created_at);
        self.touch();
    }

    pub fn touch(&mut self) {
        self.last_updated = current_timestamp();
    }
}

/// Current Unix timestamp in seconds.
pub fn current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_creation() {
        let feature = Feature::new(
            "auth".to_string(),
            "Authentication".to_string(),
            "security".to_string(),
            "User authentication system with JWT tokens".to_string(),
        );

        assert_eq!(feature.id, "auth");
        assert_eq!(feature.name, "Authentication");
        assert_eq!(feature.domain, "security");
        assert_eq!(
            feature.description,
            "User authentication system with JWT tokens"
        );
        assert_eq!(feature.status, FeatureStatus::Pending);
        assert!(feature.tags.is_empty());
        assert!(feature.context.is_none());
        assert!(feature.files_involved.is_empty());
        assert!(!feature.is_complete());
    }

    #[test]
    fn test_feature_mark_complete() {
        let mut feature = Feature::new(
            "auth".to_string(),
            "Authentication".to_string(),
            "security".to_string(),
            "User authentication system".to_string(),
        );

        feature.mark_complete();

        assert_eq!(feature.status, FeatureStatus::Complete);
        assert!(feature.is_complete());
    }

    #[test]
    fn test_legend_state() {
        let mut state = LegendState::new("My Project".to_string());

        assert_eq!(state.project_name, "My Project");
        assert!(state.features.is_empty());

        let feature = Feature::new(
            "auth".to_string(),
            "Authentication".to_string(),
            "security".to_string(),
            "User authentication system".to_string(),
        );
        state.add_feature(feature);

        assert_eq!(state.features.len(), 1);

        let found = state.find_feature("auth");
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "Authentication");
        assert_eq!(found.unwrap().domain, "security");

        let not_found = state.find_feature("nonexistent");
        assert!(not_found.is_none());
    }
}
