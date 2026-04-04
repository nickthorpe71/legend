/// Graph-backed dynamic keyword cache.
///
/// Scans the knowledge graph for `kind == "Keyword"` nodes with labels
/// like `kw:<category>:<term>` and builds per-category keyword lists.
/// Falls back to static arrays from `keywords.rs` when graph categories are empty.

use super::lexicon::{
    ACTION_KEYWORDS, ARCHITECTURE_KEYWORDS, BUG_KEYWORDS, CODE_KEYWORDS, DECISION_KEYWORDS,
    DOMAIN_KEYWORDS, ENTITY_KIND_PRIORITY, ENVIRONMENT_KEYWORDS, NEGATIVE_VALENCE_KEYWORDS,
    POSITIVE_VALENCE_KEYWORDS, PREFERENCE_KEYWORDS, TODO_KEYWORDS, TOOL_KEYWORDS,
    URGENCY_KEYWORDS,
};
use crate::memory::GraphMemory;

/// Cached keyword lists per category, built from graph + static fallbacks.
#[derive(Debug, Clone)]
pub struct KeywordCache {
    /// Code keywords: (trigger, entity_kind, context, priority) tuples.
    /// Priority determines node kind specificity during graph deduplication.
    pub code: Vec<(String, String, String, u8)>,
    pub decision: Vec<String>,
    pub action: Vec<(String, String)>,
    pub environment: Vec<String>,
    pub tool: Vec<String>,
    pub architecture: Vec<String>,
    pub bug: Vec<String>,
    pub todo: Vec<String>,
    pub preference: Vec<String>,
    /// Amygdala negative valence: (keyword, weight) for threat detection.
    pub negative_valence: Vec<(String, f32)>,
    /// Amygdala positive valence: (keyword, weight) for reward detection.
    pub positive_valence: Vec<(String, f32)>,
    /// Urgency keywords that amplify emotional magnitude.
    pub urgency: Vec<String>,
    /// Domain-specific keywords learned from workspace (Layer 2/3).
    pub domain: Vec<String>,
}

impl Default for KeywordCache {
    fn default() -> Self {
        Self::default_from_static()
    }
}

impl KeywordCache {
    /// Build cache entirely from hardcoded static arrays (for tests + backward compat).
    pub fn default_from_static() -> Self {
        Self {
            code: CODE_KEYWORDS
                .iter()
                .map(|(kw, kind, ctx, pri)| (kw.to_string(), kind.to_string(), ctx.to_string(), *pri))
                .collect(),
            decision: DECISION_KEYWORDS.iter().map(|s| s.to_string()).collect(),
            action: ACTION_KEYWORDS
                .iter()
                .map(|(v, k)| (v.to_string(), k.to_string()))
                .collect(),
            environment: ENVIRONMENT_KEYWORDS.iter().map(|s| s.to_string()).collect(),
            tool: TOOL_KEYWORDS.iter().map(|s| s.to_string()).collect(),
            architecture: ARCHITECTURE_KEYWORDS.iter().map(|s| s.to_string()).collect(),
            bug: BUG_KEYWORDS.iter().map(|s| s.to_string()).collect(),
            todo: TODO_KEYWORDS.iter().map(|s| s.to_string()).collect(),
            preference: PREFERENCE_KEYWORDS.iter().map(|s| s.to_string()).collect(),
            negative_valence: NEGATIVE_VALENCE_KEYWORDS
                .iter()
                .map(|(kw, w)| (kw.to_string(), *w))
                .collect(),
            positive_valence: POSITIVE_VALENCE_KEYWORDS
                .iter()
                .map(|(kw, w)| (kw.to_string(), *w))
                .collect(),
            urgency: URGENCY_KEYWORDS.iter().map(|s| s.to_string()).collect(),
            domain: DOMAIN_KEYWORDS.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Scan the knowledge graph for `kind == "Keyword"` nodes and build cache.
    /// Each keyword node has label `kw:<category>:<term>` and optional metadata
    /// in `source_texts` for code keywords (`entity_kind:X`, `entity_context:Y`).
    pub fn from_graph(graph: &GraphMemory) -> Self {
        let mut cache = Self {
            code: Vec::new(),
            decision: Vec::new(),
            action: Vec::new(),
            environment: Vec::new(),
            tool: Vec::new(),
            architecture: Vec::new(),
            bug: Vec::new(),
            todo: Vec::new(),
            preference: Vec::new(),
            negative_valence: Vec::new(),
            positive_valence: Vec::new(),
            urgency: Vec::new(),
            domain: Vec::new(),
        };

        for node in graph.nodes.values() {
            if node.kind != "Keyword" {
                continue;
            }
            if let Some((category, term)) = parse_keyword_label(&node.label) {
                match category {
                    "code" => {
                        let (entity_kind, entity_context, priority) =
                            parse_code_metadata(&node.source_texts);
                        cache.code.push((term.to_string(), entity_kind, entity_context, priority));
                    }
                    "decision" => cache.decision.push(term.to_string()),
                    "action" => cache.action.push((term.to_string(), "Action".to_string())),
                    "environment" => cache.environment.push(term.to_string()),
                    "tool" => cache.tool.push(term.to_string()),
                    "architecture" => cache.architecture.push(term.to_string()),
                    "bug" => cache.bug.push(term.to_string()),
                    "todo" => cache.todo.push(term.to_string()),
                    "preference" => cache.preference.push(term.to_string()),
                    "negative_valence" => {
                        let weight = parse_valence_weight(&node.source_texts, -0.4);
                        cache.negative_valence.push((term.to_string(), weight));
                    }
                    "positive_valence" => {
                        let weight = parse_valence_weight(&node.source_texts, 0.4);
                        cache.positive_valence.push((term.to_string(), weight));
                    }
                    "urgency" => cache.urgency.push(term.to_string()),
                    "domain" => cache.domain.push(term.to_string()),
                    _ => {} // Unknown category, skip
                }
            }
        }

        cache.apply_fallbacks();
        cache
    }

    /// Look up the priority for an entity kind.
    ///
    /// Checks code keywords first (their kind → priority mapping), then falls
    /// back to `ENTITY_KIND_PRIORITY` for non-code kinds. Returns 1 for unknown kinds.
    pub fn kind_priority(&self, kind: &str) -> u8 {
        // Check code keywords — each has a (trigger, entity_kind, context, priority)
        for (_, ek, _, pri) in &self.code {
            if ek == kind {
                return *pri;
            }
        }
        // Fall back to static non-code priority table
        for (ek, pri) in ENTITY_KIND_PRIORITY {
            if *ek == kind {
                return *pri;
            }
        }
        1 // unknown kind = lowest priority
    }

    /// Check if the given (lowercased) text matches any classification keyword.
    pub fn matches_any_category(&self, text: &str) -> bool {
        [&self.decision, &self.bug, &self.todo, &self.architecture,
         &self.preference, &self.tool, &self.domain]
            .iter()
            .any(|list| list.iter().any(|k| text.contains(k.as_str())))
    }

    /// Fill empty categories from static `keywords.rs` arrays.
    pub fn apply_fallbacks(&mut self) {
        if self.code.is_empty() {
            self.code = CODE_KEYWORDS
                .iter()
                .map(|(kw, kind, ctx, pri)| (kw.to_string(), kind.to_string(), ctx.to_string(), *pri))
                .collect();
        }
        if self.decision.is_empty() {
            self.decision = DECISION_KEYWORDS.iter().map(|s| s.to_string()).collect();
        }
        if self.action.is_empty() {
            self.action = ACTION_KEYWORDS
                .iter()
                .map(|(v, k)| (v.to_string(), k.to_string()))
                .collect();
        }
        if self.environment.is_empty() {
            self.environment = ENVIRONMENT_KEYWORDS.iter().map(|s| s.to_string()).collect();
        }
        if self.tool.is_empty() {
            self.tool = TOOL_KEYWORDS.iter().map(|s| s.to_string()).collect();
        }
        if self.architecture.is_empty() {
            self.architecture = ARCHITECTURE_KEYWORDS.iter().map(|s| s.to_string()).collect();
        }
        if self.bug.is_empty() {
            self.bug = BUG_KEYWORDS.iter().map(|s| s.to_string()).collect();
        }
        if self.todo.is_empty() {
            self.todo = TODO_KEYWORDS.iter().map(|s| s.to_string()).collect();
        }
        if self.preference.is_empty() {
            self.preference = PREFERENCE_KEYWORDS.iter().map(|s| s.to_string()).collect();
        }
        if self.negative_valence.is_empty() {
            self.negative_valence = NEGATIVE_VALENCE_KEYWORDS
                .iter()
                .map(|(kw, w)| (kw.to_string(), *w))
                .collect();
        }
        if self.positive_valence.is_empty() {
            self.positive_valence = POSITIVE_VALENCE_KEYWORDS
                .iter()
                .map(|(kw, w)| (kw.to_string(), *w))
                .collect();
        }
        if self.urgency.is_empty() {
            self.urgency = URGENCY_KEYWORDS.iter().map(|s| s.to_string()).collect();
        }
        if self.domain.is_empty() {
            self.domain = DOMAIN_KEYWORDS.iter().map(|s| s.to_string()).collect();
        }
    }
}

/// Parse a keyword label like `kw:decision:because` into (category, term).
fn parse_keyword_label(label: &str) -> Option<(&str, &str)> {
    let rest = label.strip_prefix("kw:")?;
    let colon_pos = rest.find(':')?;
    let category = &rest[..colon_pos];
    let term = &rest[colon_pos + 1..];
    if term.is_empty() {
        return None;
    }
    Some((category, term))
}

/// Parse code keyword metadata from source_texts.
/// Looks for `entity_kind:X`, `entity_context:Y`, and `entity_priority:N` entries.
fn parse_code_metadata(source_texts: &[String]) -> (String, String, u8) {
    let mut kind = "Symbol".to_string();
    let mut context = "mentions".to_string();
    let mut priority: u8 = 1; // default lowest

    for text in source_texts {
        if let Some(k) = text.strip_prefix("entity_kind:") {
            kind = k.to_string();
        } else if let Some(c) = text.strip_prefix("entity_context:") {
            context = c.to_string();
        } else if let Some(p) = text.strip_prefix("entity_priority:") {
            priority = p.parse().unwrap_or(1);
        }
    }

    (kind, context, priority)
}

/// Parse valence weight from source_texts.
/// Looks for `weight:X` entry; returns `default` if not found.
fn parse_valence_weight(source_texts: &[String], default: f32) -> f32 {
    for text in source_texts {
        if let Some(w) = text.strip_prefix("weight:") {
            if let Ok(val) = w.parse::<f32>() {
                return val.clamp(-1.0, 1.0);
            }
        }
    }
    default
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{GraphMemory, GraphNode};
    #[test]
    fn test_default_from_static_not_empty() {
        let cache = KeywordCache::default_from_static();
        assert!(!cache.code.is_empty());
        assert!(!cache.decision.is_empty());
        assert!(!cache.action.is_empty());
        assert!(!cache.environment.is_empty());
        // tool and domain are intentionally empty in static — populated via Layer 2/3
        assert!(cache.tool.is_empty(), "static tools should be empty");
        assert!(cache.domain.is_empty(), "static domain should be empty");
        assert!(!cache.architecture.is_empty());
        assert!(!cache.bug.is_empty());
        assert!(!cache.todo.is_empty());
        assert!(!cache.preference.is_empty());
        assert!(!cache.negative_valence.is_empty());
        assert!(!cache.positive_valence.is_empty());
        assert!(!cache.urgency.is_empty());
    }

    #[test]
    fn test_empty_graph_uses_defaults() {
        let graph = GraphMemory::default();
        let cache = KeywordCache::from_graph(&graph);
        let static_cache = KeywordCache::default_from_static();
        assert_eq!(cache.decision.len(), static_cache.decision.len());
        assert_eq!(cache.code.len(), static_cache.code.len());
    }

    #[test]
    fn test_graph_nodes_populate_cache() {
        let mut graph = GraphMemory::default();
        graph.nodes.insert(
            1,
            GraphNode {
                id: 1,
                label: "kw:tool:bevy".to_string(),
                kind: "Keyword".to_string(),
                weight: 1.0,
                last_seen: 10,
                salience: 0.5,
                source_texts: vec![],
                embedding: Vec::new(),
                full_text: None,
            },
        );
        graph.nodes.insert(
            2,
            GraphNode {
                id: 2,
                label: "kw:decision:tradeoff".to_string(),
                kind: "Keyword".to_string(),
                weight: 1.0,
                last_seen: 10,
                salience: 0.5,
                source_texts: vec![],
                embedding: Vec::new(),
                full_text: None,
            },
        );

        let cache = KeywordCache::from_graph(&graph);
        assert!(cache.tool.contains(&"bevy".to_string()));
        assert!(cache.decision.contains(&"tradeoff".to_string()));
        // Decision has graph keyword + static fallbacks should NOT apply (category not empty)
        // Actually, decision has 1 entry from graph, so fallbacks should NOT apply
        assert_eq!(cache.decision.len(), 1);
    }

    #[test]
    fn test_mixed_fallback_per_category() {
        let mut graph = GraphMemory::default();
        // Only populate tool category from graph
        graph.nodes.insert(
            1,
            GraphNode {
                id: 1,
                label: "kw:tool:prisma".to_string(),
                kind: "Keyword".to_string(),
                weight: 1.0,
                last_seen: 10,
                salience: 0.5,
                source_texts: vec![],
                embedding: Vec::new(),
                full_text: None,
            },
        );

        let cache = KeywordCache::from_graph(&graph);
        // Tool populated from graph
        assert!(cache.tool.contains(&"prisma".to_string()));
        assert_eq!(cache.tool.len(), 1);
        // Decision falls back to static
        assert_eq!(
            cache.decision.len(),
            DECISION_KEYWORDS.len()
        );
    }

    #[test]
    fn test_code_keyword_with_metadata() {
        let mut graph = GraphMemory::default();
        graph.nodes.insert(
            1,
            GraphNode {
                id: 1,
                label: "kw:code:async fn ".to_string(),
                kind: "Keyword".to_string(),
                weight: 1.0,
                last_seen: 10,
                salience: 0.5,
                source_texts: vec![
                    "entity_kind:Function".to_string(),
                    "entity_context:defines".to_string(),
                ],
                embedding: Vec::new(),
                full_text: None,
            },
        );

        let cache = KeywordCache::from_graph(&graph);
        assert_eq!(cache.code.len(), 1);
        assert_eq!(cache.code[0].0, "async fn ");
        assert_eq!(cache.code[0].1, "Function");
        assert_eq!(cache.code[0].2, "defines");
    }

    #[test]
    fn test_parse_keyword_label() {
        assert_eq!(
            parse_keyword_label("kw:decision:because"),
            Some(("decision", "because"))
        );
        assert_eq!(
            parse_keyword_label("kw:tool:prisma"),
            Some(("tool", "prisma"))
        );
        assert_eq!(parse_keyword_label("not_a_keyword"), None);
        assert_eq!(parse_keyword_label("kw:decision:"), None);
        assert_eq!(parse_keyword_label("kw:"), None);
    }

    #[test]
    fn test_non_keyword_nodes_ignored() {
        let mut graph = GraphMemory::default();
        graph.nodes.insert(
            1,
            GraphNode {
                id: 1,
                label: "some_function".to_string(),
                kind: "Function".to_string(),
                weight: 2.0,
                last_seen: 10,
                salience: 0.8,
                source_texts: vec![],
                embedding: Vec::new(),
                full_text: None,
            },
        );

        let cache = KeywordCache::from_graph(&graph);
        // All categories should fall back to static defaults
        assert_eq!(cache.decision.len(), DECISION_KEYWORDS.len());
    }

    #[test]
    fn test_parse_valence_weight() {
        assert_eq!(parse_valence_weight(&["weight:-0.6".to_string()], -0.4), -0.6);
        assert_eq!(parse_valence_weight(&["weight:0.8".to_string()], 0.4), 0.8);
        assert_eq!(parse_valence_weight(&[], -0.4), -0.4);
        assert_eq!(parse_valence_weight(&["no_weight".to_string()], 0.4), 0.4);
        // Clamped to [-1, 1]
        assert_eq!(parse_valence_weight(&["weight:5.0".to_string()], 0.0), 1.0);
        assert_eq!(parse_valence_weight(&["weight:-5.0".to_string()], 0.0), -1.0);
    }

    #[test]
    fn test_valence_graph_nodes() {
        let mut graph = GraphMemory::default();
        graph.nodes.insert(
            1,
            GraphNode {
                id: 1,
                label: "kw:negative_valence:meltdown".to_string(),
                kind: "Keyword".to_string(),
                weight: 1.0,
                last_seen: 10,
                salience: 0.5,
                source_texts: vec!["weight:-0.7".to_string()],
                embedding: Vec::new(),
                full_text: None,
            },
        );
        graph.nodes.insert(
            2,
            GraphNode {
                id: 2,
                label: "kw:positive_valence:breakthrough".to_string(),
                kind: "Keyword".to_string(),
                weight: 1.0,
                last_seen: 10,
                salience: 0.5,
                source_texts: vec!["weight:0.6".to_string()],
                embedding: Vec::new(),
                full_text: None,
            },
        );
        graph.nodes.insert(
            3,
            GraphNode {
                id: 3,
                label: "kw:urgency:showstopper".to_string(),
                kind: "Keyword".to_string(),
                weight: 1.0,
                last_seen: 10,
                salience: 0.5,
                source_texts: vec![],
                embedding: Vec::new(),
                full_text: None,
            },
        );

        let cache = KeywordCache::from_graph(&graph);
        // Graph entries populate the categories (no static fallback)
        assert_eq!(cache.negative_valence.len(), 1);
        assert_eq!(cache.negative_valence[0].0, "meltdown");
        assert_eq!(cache.negative_valence[0].1, -0.7);
        assert_eq!(cache.positive_valence.len(), 1);
        assert_eq!(cache.positive_valence[0].0, "breakthrough");
        assert_eq!(cache.positive_valence[0].1, 0.6);
        assert_eq!(cache.urgency.len(), 1);
        assert!(cache.urgency.contains(&"showstopper".to_string()));
    }

    #[test]
    fn test_empty_graph_valence_fallbacks() {
        let graph = GraphMemory::default();
        let cache = KeywordCache::from_graph(&graph);
        assert_eq!(cache.negative_valence.len(), NEGATIVE_VALENCE_KEYWORDS.len());
        assert_eq!(cache.positive_valence.len(), POSITIVE_VALENCE_KEYWORDS.len());
        assert_eq!(cache.urgency.len(), URGENCY_KEYWORDS.len());
    }
}
