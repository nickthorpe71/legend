// Core domain types for Legend
//
// R* principle: Flat, simple structs with public fields
// No builders, no complex constructors - just data

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

// FeatureStatus enum
//
// Rust enums are powerful - not just integers like C
// Each variant is a distinct type-safe value
// The compiler ensures we handle all cases in match expressions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FeatureStatus {
    Pending,
    InProgress,
    Blocked,
    Complete,
}

// Feature - represents a single feature being tracked
//
// R* principle: Public fields for simple data structures
// No getters/setters - direct field access is clearer
//
// Performance note: These fields enable fast filtering (domain/tags)
// and rich context (description/context) for AI understanding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feature {
    // Identity
    pub id: String,                  // Unique identifier (e.g., "auth-login")
    pub name: String,                // Human-readable name

    // Categorization (for fast filtering)
    pub domain: String,              // Primary category: "auth", "storage", "api", "ui"
    pub tags: Vec<String>,           // Flexible labels: ["backend", "security", "database"]
    pub status: FeatureStatus,       // Current status

    // Rich context (for AI understanding)
    pub description: String,         // What this feature does (used for embeddings)
    pub context: Option<String>,     // Why we're building it, background (optional)

    // File tracking
    pub files_involved: Vec<String>, // Files related to this feature

    // Temporal metadata
    pub created_at: i64,             // Unix timestamp (seconds since epoch)
    pub last_updated: i64,           // Unix timestamp
    pub recency_score: f64,          // For temporal weighting (1.0 = most recent)
}

// impl block - adds methods to Feature
impl Feature {
    // Associated function (like a static method in other languages)
    // Called as: Feature::new(...)
    //
    // Why not a builder? R* principle: Simple constructors for simple types
    // We require the essential fields (id, name, domain, description)
    // Optional/default fields are set automatically
    pub fn new(id: String, name: String, domain: String, description: String) -> Self {
        let now = current_timestamp();

        Feature {
            id,
            name,
            domain,
            description,
            status: FeatureStatus::Pending,
            tags: Vec::new(),           // Start with no tags
            context: None,              // Optional context
            files_involved: Vec::new(),
            created_at: now,
            last_updated: now,
            recency_score: 1.0, // New features start with max recency
        }
    }

    // Method that borrows self (read-only)
    // Called as: feature.is_complete()
    pub fn is_complete(&self) -> bool {
        self.status == FeatureStatus::Complete
    }

    // Method that mutably borrows self (can modify)
    // Called as: feature.touch()
    pub fn touch(&mut self) {
        self.last_updated = current_timestamp();
    }

    // Method that mutably borrows self
    pub fn mark_complete(&mut self) {
        self.status = FeatureStatus::Complete;
        self.touch();
    }
}

// LegendState - the entire state of Legend for a project
//
// This is what gets saved to disk and loaded back
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegendState {
    pub project_name: String,
    pub features: Vec<Feature>,
    pub created_at: i64,
    pub last_updated: i64,
}

impl LegendState {
    // Create a new empty state
    pub fn new(project_name: String) -> Self {
        let now = current_timestamp();

        LegendState {
            project_name,
            features: Vec::new(),
            created_at: now,
            last_updated: now,
        }
    }

    // Add a feature to the state
    pub fn add_feature(&mut self, feature: Feature) {
        self.features.push(feature);
        self.touch();
    }

    // Find a feature by ID (returns Option because it might not exist)
    // Why Option<&Feature>? We're returning a reference (borrow), not ownership
    // Option because the feature might not be found
    pub fn find_feature(&self, id: &str) -> Option<&Feature> {
        // Iterator pattern: find the first feature with matching ID
        self.features.iter().find(|f| f.id == id)
    }

    // Find a feature mutably (so caller can modify it)
    pub fn find_feature_mut(&mut self, id: &str) -> Option<&mut Feature> {
        self.features.iter_mut().find(|f| f.id == id)
    }

    // Update the last_updated timestamp
    pub fn touch(&mut self) {
        self.last_updated = current_timestamp();
    }
}

// Helper function to get current Unix timestamp
// Not a method - just a utility function
fn current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs() as i64
}

// Tests live with the code they test
// Run with: cargo test
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
        assert_eq!(feature.description, "User authentication system with JWT tokens");
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
