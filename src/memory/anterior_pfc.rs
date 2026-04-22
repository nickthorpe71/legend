/// Anterior Prefrontal Cortex (BA10) — Prospective Memory & Plan Management
///
/// Maintains pending goals and structured plans as an executive queue WITHOUT
/// consuming working memory or short-term episodic memory. Plans persist inside
/// the same serialized memory state, but they are read through start/query
/// executive pathways instead of the hippocampal L1/L2/L3 encoding path.
///
/// Key mechanisms:
/// - Cognitive branching (Koechlin): suspended goals in compressed form
/// - Implementation intentions (Gollwitzer): structured items improve execution
/// - ACC-inspired lifecycle: Active → Completed inside the executive queue
use serde::{Deserialize, Serialize};

use super::entorhinal::{cosine_similarity, embed_text};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Cosine similarity threshold for fuzzy plan name matching.
/// Set high (0.95) to avoid false matches between plans with similar prefixes
/// (e.g., "Phase 2 Review" vs "Phase 3 Overhaul"). Exact case-insensitive
/// match is checked first, so this only fires for genuine rephrasings.
pub const PLAN_NAME_MATCH_THRESHOLD: f32 = 0.95;

/// Cosine similarity threshold for spontaneous retrieval of plan items during query.
pub const INTENTION_CUE_THRESHOLD: f32 = 0.40;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ItemStatus {
    Active,
    Pending,
    Deferred,
    Done,
}

impl ItemStatus {
    pub fn label(&self) -> &'static str {
        match self {
            ItemStatus::Active => "active",
            ItemStatus::Pending => "pending",
            ItemStatus::Deferred => "deferred",
            ItemStatus::Done => "done",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanItem {
    pub text: String,
    pub status: ItemStatus,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: u64,
    pub name: String,
    pub items: Vec<PlanItem>,
    pub created_at: u64,
    pub updated_at: u64,
    pub completed_at: Option<u64>,
}

impl Plan {
    /// Check if all items are Done and update completed_at accordingly.
    pub fn update_completion_status(&mut self, clock: u64) {
        if self.items.is_empty() {
            return;
        }
        let all_done = self.items.iter().all(|i| i.status == ItemStatus::Done);
        if all_done && self.completed_at.is_none() {
            self.completed_at = Some(clock);
        } else if !all_done {
            self.completed_at = None;
        }
    }
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse a PLAN: tick into a plan name and items.
///
/// Format:
/// ```text
/// PLAN: Plan Name
/// [active] Item description
/// [pending] Another item
/// [deferred] Postponed item
/// [done] Completed item
/// Unmarked line defaults to Pending
/// ```
pub fn parse_plan_text(text: &str) -> Option<(String, Vec<(String, ItemStatus)>)> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return None;
    }

    // First line = plan name (strip "PLAN:" prefix if still present)
    let name_line = lines[0].trim();
    let name = if let Some(rest) = name_line.strip_prefix("PLAN:") {
        rest.trim().to_string()
    } else {
        name_line.to_string()
    };

    if name.is_empty() {
        return None;
    }

    let mut items = Vec::new();
    for line in &lines[1..] {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let (status, item_text) = if let Some(rest) = trimmed.strip_prefix("[active]") {
            (ItemStatus::Active, rest.trim())
        } else if let Some(rest) = trimmed.strip_prefix("[pending]") {
            (ItemStatus::Pending, rest.trim())
        } else if let Some(rest) = trimmed.strip_prefix("[deferred]") {
            (ItemStatus::Deferred, rest.trim())
        } else if let Some(rest) = trimmed.strip_prefix("[done]") {
            (ItemStatus::Done, rest.trim())
        } else {
            // Lines without status markers default to Pending
            (ItemStatus::Pending, trimmed)
        };

        if !item_text.is_empty() {
            items.push((item_text.to_string(), status));
        }
    }

    Some((name, items))
}

/// Strip the PLAN: prefix from tick text, returning the remaining text for parsing.
pub fn strip_plan_prefix(text: &str) -> Option<&str> {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix("PLAN:") {
        Some(rest.trim_start())
    } else {
        None
    }
}

/// Find a matching plan by name (exact case-insensitive, then fuzzy embedding match).
pub fn find_matching_plan(plans: &[Plan], name: &str, embedding_dim: usize) -> Option<usize> {
    // 1. Exact name match (case-insensitive)
    let name_lower = name.to_lowercase();
    if let Some(idx) = plans
        .iter()
        .position(|p| p.name.to_lowercase() == name_lower)
    {
        return Some(idx);
    }

    // 2. Fuzzy match via embedding similarity
    let name_embedding = embed_text(name, embedding_dim);
    let mut best_idx = None;
    let mut best_sim = PLAN_NAME_MATCH_THRESHOLD;
    for (i, plan) in plans.iter().enumerate() {
        let plan_name_embedding = embed_text(&plan.name, embedding_dim);
        let sim = cosine_similarity(&name_embedding, &plan_name_embedding);
        if sim > best_sim {
            best_sim = sim;
            best_idx = Some(i);
        }
    }
    best_idx
}

/// Apply a parsed plan (name + items) to the plans register.
/// Returns the plan ID (new or existing).
pub fn apply_plan(
    plans: &mut Vec<Plan>,
    name: String,
    parsed_items: Vec<(String, ItemStatus)>,
    clock: u64,
    next_id: &mut u64,
    embedding_dim: usize,
) -> u64 {
    let items: Vec<PlanItem> = parsed_items
        .into_iter()
        .map(|(text, status)| {
            let embedding = embed_text(&text, embedding_dim);
            PlanItem {
                text,
                status,
                embedding,
            }
        })
        .collect();

    if let Some(idx) = find_matching_plan(plans, &name, embedding_dim) {
        // Update existing plan in-place (full-state write)
        let plan = &mut plans[idx];
        plan.items = items;
        plan.updated_at = clock;
        plan.update_completion_status(clock);
        plan.id
    } else {
        // Create new plan
        let id = *next_id;
        *next_id += 1;
        let mut plan = Plan {
            id,
            name,
            items,
            created_at: clock,
            updated_at: clock,
            completed_at: None,
        };
        plan.update_completion_status(clock);
        plans.push(plan);
        id
    }
}

// ---------------------------------------------------------------------------
// Navigation
// ---------------------------------------------------------------------------

/// Find the next actionable plan item across all active plans.
/// Returns `(plan_name, item_text)` for the first Active item found,
/// falling back to the first Pending item if no Active items exist.
pub fn find_next_plan_action(plans: &[Plan]) -> Option<(String, String)> {
    // Priority 1: first Active item in any non-completed plan
    for plan in plans.iter().filter(|p| p.completed_at.is_none()) {
        if let Some(item) = plan.items.iter().find(|i| i.status == ItemStatus::Active) {
            return Some((plan.name.clone(), item.text.clone()));
        }
    }
    // Priority 2: first Pending item in any non-completed plan
    for plan in plans.iter().filter(|p| p.completed_at.is_none()) {
        if let Some(item) = plan.items.iter().find(|i| i.status == ItemStatus::Pending) {
            return Some((plan.name.clone(), item.text.clone()));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

/// Format plans for the start summary display.
pub fn format_plans_for_summary(plans: &[Plan]) -> serde_json::Value {
    if plans.is_empty() {
        return serde_json::Value::Null;
    }

    let mut active_plans = Vec::new();
    let mut completed_plans = Vec::new();

    for plan in plans {
        let items: Vec<serde_json::Value> = plan
            .items
            .iter()
            .map(|item| {
                serde_json::json!({
                    "text": item.text,
                    "status": item.status.label(),
                })
            })
            .collect();

        let plan_json = serde_json::json!({
            "name": plan.name,
            "items": items,
            "completed": plan.completed_at.is_some(),
        });

        if plan.completed_at.is_some() {
            completed_plans.push(plan_json);
        } else {
            active_plans.push(plan_json);
        }
    }

    serde_json::json!({
        "active": active_plans,
        "completed": completed_plans,
    })
}

/// Format plans for plain-text display in internal tools and tests.
#[cfg(test)]
pub fn format_plans_cli(plans: &[Plan]) -> String {
    if plans.is_empty() {
        return "No current plans.".to_string();
    }

    let mut out = String::new();
    for (i, plan) in plans.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let status_marker = if plan.completed_at.is_some() {
            " (completed)"
        } else {
            ""
        };
        out.push_str(&format!("Plan: {}{}\n", plan.name, status_marker));
        for item in &plan.items {
            out.push_str(&format!("  [{}] {}\n", item.status.label(), item.text));
        }
    }
    out
}

/// Build a compact session-log entry for PLAN ticks without storing the whole
/// plan body as recent episodic activity.
pub fn summarize_plan_tick(text: &str) -> Option<String> {
    let plan_body = strip_plan_prefix(text)?;
    let (name, items) = parse_plan_text(plan_body)?;

    let mut active = 0;
    let mut pending = 0;
    let mut deferred = 0;
    let mut done = 0;
    for (_, status) in items {
        match status {
            ItemStatus::Active => active += 1,
            ItemStatus::Pending => pending += 1,
            ItemStatus::Deferred => deferred += 1,
            ItemStatus::Done => done += 1,
        }
    }

    Some(format!(
        "PLAN updated: {} (active: {}, pending: {}, deferred: {}, done: {})",
        name, active, pending, deferred, done
    ))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_plan_basic() {
        let text =
            "PLAN: Test Plan\n[active] Fix the bug\n[deferred] Optimize later\n[done] Write tests";
        let (name, items) = parse_plan_text(text).unwrap();
        assert_eq!(name, "Test Plan");
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].0, "Fix the bug");
        assert_eq!(items[0].1, ItemStatus::Active);
        assert_eq!(items[1].0, "Optimize later");
        assert_eq!(items[1].1, ItemStatus::Deferred);
        assert_eq!(items[2].0, "Write tests");
        assert_eq!(items[2].1, ItemStatus::Done);
    }

    #[test]
    fn test_parse_plan_defaults_pending() {
        let text = "My Plan\nUnmarked item one\nUnmarked item two";
        let (name, items) = parse_plan_text(text).unwrap();
        assert_eq!(name, "My Plan");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].1, ItemStatus::Pending);
        assert_eq!(items[1].1, ItemStatus::Pending);
    }

    #[test]
    fn test_parse_plan_empty_lines_ignored() {
        let text = "PLAN: Test\n\n[active] Item one\n\n[pending] Item two\n";
        let (name, items) = parse_plan_text(text).unwrap();
        assert_eq!(name, "Test");
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn test_parse_plan_empty_name_returns_none() {
        let text = "PLAN: \n[active] Item";
        assert!(parse_plan_text(text).is_none());
    }

    #[test]
    fn test_strip_plan_prefix() {
        assert_eq!(
            strip_plan_prefix("PLAN: My Plan\n[active] item"),
            Some("My Plan\n[active] item")
        );
        assert_eq!(strip_plan_prefix("DECISION: chose X"), None);
        assert_eq!(
            strip_plan_prefix("  PLAN: Leading spaces"),
            Some("Leading spaces")
        );
    }

    #[test]
    fn test_plan_completed_when_all_done() {
        let mut plan = Plan {
            id: 1,
            name: "Test".to_string(),
            items: vec![
                PlanItem {
                    text: "a".into(),
                    status: ItemStatus::Done,
                    embedding: vec![],
                },
                PlanItem {
                    text: "b".into(),
                    status: ItemStatus::Done,
                    embedding: vec![],
                },
            ],
            created_at: 1,
            updated_at: 1,
            completed_at: None,
        };
        plan.update_completion_status(10);
        assert_eq!(plan.completed_at, Some(10));
    }

    #[test]
    fn test_plan_not_completed_with_pending() {
        let mut plan = Plan {
            id: 1,
            name: "Test".to_string(),
            items: vec![
                PlanItem {
                    text: "a".into(),
                    status: ItemStatus::Done,
                    embedding: vec![],
                },
                PlanItem {
                    text: "b".into(),
                    status: ItemStatus::Pending,
                    embedding: vec![],
                },
            ],
            created_at: 1,
            updated_at: 1,
            completed_at: None,
        };
        plan.update_completion_status(10);
        assert!(plan.completed_at.is_none());
    }

    #[test]
    fn test_plan_completed_cleared_on_reactivation() {
        let mut plan = Plan {
            id: 1,
            name: "Test".to_string(),
            items: vec![PlanItem {
                text: "a".into(),
                status: ItemStatus::Active,
                embedding: vec![],
            }],
            created_at: 1,
            updated_at: 1,
            completed_at: Some(5),
        };
        plan.update_completion_status(10);
        assert!(plan.completed_at.is_none());
    }

    #[test]
    fn test_apply_plan_creates_new() {
        let mut plans = Vec::new();
        let mut next_id = 1u64;
        let id = apply_plan(
            &mut plans,
            "New Plan".to_string(),
            vec![("Item 1".to_string(), ItemStatus::Active)],
            10,
            &mut next_id,
            384,
        );
        assert_eq!(id, 1);
        assert_eq!(next_id, 2);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].name, "New Plan");
        assert_eq!(plans[0].items.len(), 1);
    }

    #[test]
    fn test_apply_plan_updates_existing_by_name() {
        let mut plans = Vec::new();
        let mut next_id = 1u64;
        apply_plan(
            &mut plans,
            "My Plan".to_string(),
            vec![("Item 1".to_string(), ItemStatus::Active)],
            10,
            &mut next_id,
            384,
        );
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].items.len(), 1);

        // Update with same name
        let id2 = apply_plan(
            &mut plans,
            "My Plan".to_string(),
            vec![
                ("Item 1".to_string(), ItemStatus::Done),
                ("Item 2".to_string(), ItemStatus::Active),
            ],
            20,
            &mut next_id,
            384,
        );
        assert_eq!(plans.len(), 1); // Still just one plan
        assert_eq!(id2, 1); // Same ID
        assert_eq!(plans[0].items.len(), 2); // Updated items
        assert_eq!(plans[0].updated_at, 20);
        assert_eq!(next_id, 2); // next_id not incremented for update
    }

    #[test]
    fn test_apply_plan_creates_new_for_different_name() {
        let mut plans = Vec::new();
        let mut next_id = 1u64;
        apply_plan(
            &mut plans,
            "Database Migration Strategy".to_string(),
            vec![("Migrate tables".to_string(), ItemStatus::Active)],
            10,
            &mut next_id,
            384,
        );
        apply_plan(
            &mut plans,
            "Frontend Redesign".to_string(),
            vec![("Redesign homepage".to_string(), ItemStatus::Pending)],
            20,
            &mut next_id,
            384,
        );
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].name, "Database Migration Strategy");
        assert_eq!(plans[1].name, "Frontend Redesign");
    }

    #[test]
    fn test_format_plans_cli() {
        let plans = vec![Plan {
            id: 1,
            name: "Test".to_string(),
            items: vec![PlanItem {
                text: "Do thing".into(),
                status: ItemStatus::Active,
                embedding: vec![],
            }],
            created_at: 1,
            updated_at: 1,
            completed_at: None,
        }];
        let output = format_plans_cli(&plans);
        assert!(output.contains("Plan: Test"));
        assert!(output.contains("[active] Do thing"));
    }

    #[test]
    fn test_summarize_plan_tick_omits_body() {
        let summary = summarize_plan_tick(
            "PLAN: Test Plan\n[active] Fix parser\n[pending] Write tests\n[done] Inspect path",
        )
        .unwrap();
        assert_eq!(
            summary,
            "PLAN updated: Test Plan (active: 1, pending: 1, deferred: 0, done: 1)"
        );
        assert!(!summary.contains("Fix parser"));
    }

    #[test]
    fn test_find_next_plan_action_active_first() {
        let plans = vec![Plan {
            id: 1,
            name: "My Plan".to_string(),
            items: vec![
                PlanItem {
                    text: "Done thing".into(),
                    status: ItemStatus::Done,
                    embedding: vec![],
                },
                PlanItem {
                    text: "Active thing".into(),
                    status: ItemStatus::Active,
                    embedding: vec![],
                },
                PlanItem {
                    text: "Pending thing".into(),
                    status: ItemStatus::Pending,
                    embedding: vec![],
                },
            ],
            created_at: 1,
            updated_at: 1,
            completed_at: None,
        }];
        let (name, text) = find_next_plan_action(&plans).unwrap();
        assert_eq!(name, "My Plan");
        assert_eq!(text, "Active thing");
    }

    #[test]
    fn test_find_next_plan_action_falls_back_to_pending() {
        let plans = vec![Plan {
            id: 1,
            name: "Plan B".to_string(),
            items: vec![
                PlanItem {
                    text: "Done".into(),
                    status: ItemStatus::Done,
                    embedding: vec![],
                },
                PlanItem {
                    text: "Next pending".into(),
                    status: ItemStatus::Pending,
                    embedding: vec![],
                },
            ],
            created_at: 1,
            updated_at: 1,
            completed_at: None,
        }];
        let (name, text) = find_next_plan_action(&plans).unwrap();
        assert_eq!(name, "Plan B");
        assert_eq!(text, "Next pending");
    }

    #[test]
    fn test_find_next_plan_action_skips_completed_plans() {
        let plans = vec![Plan {
            id: 1,
            name: "Done Plan".to_string(),
            items: vec![PlanItem {
                text: "Active".into(),
                status: ItemStatus::Active,
                embedding: vec![],
            }],
            created_at: 1,
            updated_at: 1,
            completed_at: Some(5),
        }];
        assert!(find_next_plan_action(&plans).is_none());
    }

    #[test]
    fn test_find_next_plan_action_empty() {
        assert!(find_next_plan_action(&[]).is_none());
    }

    #[test]
    fn test_item_status_labels() {
        assert_eq!(ItemStatus::Active.label(), "active");
        assert_eq!(ItemStatus::Pending.label(), "pending");
        assert_eq!(ItemStatus::Deferred.label(), "deferred");
        assert_eq!(ItemStatus::Done.label(), "done");
    }
}
