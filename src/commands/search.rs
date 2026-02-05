// Search command - find features by keyword, domain, tags, or status
//
// This enables Claude to quickly find relevant features when a user
// says something like "lets work on auth" - Claude runs:
//   legend search auth
// and gets back matching features with full context
//
// Rust concepts in this file:
// - String matching with contains() and to_lowercase()
// - Combining filters with iterators
// - Collecting filtered results into a Vec
// - Command-line argument handling

use crate::storage;
use crate::types::Feature;

/// Handle the search command
///
/// Usage:
///   legend search <query>             - search all fields
///   legend search --domain <domain>   - filter by domain
///   legend search --tag <tag>         - filter by tag
///   legend search --status <status>   - filter by status
///
/// Flags can be combined:
///   legend search auth --domain security --status Pending
///
/// Output: JSON array of matching features (for Claude)
pub fn handle_search(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.is_empty() {
        return Err("Usage: legend search <query> [--domain <d>] [--tag <t>] [--status <s>]".into());
    }

    // Parse arguments into a SearchQuery
    let query = parse_args(args)?;

    // Load state
    let state = storage::load_state()?;

    // Filter features based on query
    // This uses iterator chaining - each .filter() narrows the results
    let results: Vec<&Feature> = state
        .features
        .iter()
        .filter(|f| matches_query(f, &query))
        .collect();

    if results.is_empty() {
        println!("[]");
        eprintln!("No features matched the search.");
        return Ok(());
    }

    // Output as JSON for Claude to consume
    let json = serde_json::to_string_pretty(&results)
        .map_err(|e| format!("Failed to serialize results: {}", e))?;

    println!("{}", json);
    eprintln!("Found {} matching feature(s).", results.len());

    Ok(())
}

/// Parsed search query with optional filters
struct SearchQuery {
    /// Free-text keyword to match against id, name, description, context
    keyword: Option<String>,
    /// Filter by domain
    domain: Option<String>,
    /// Filter by tag
    tag: Option<String>,
    /// Filter by status (as string, matched case-insensitively)
    status: Option<String>,
}

/// Parse command-line args into a SearchQuery
///
/// Handles both positional keyword and --flag arguments
fn parse_args(args: &[String]) -> Result<SearchQuery, Box<dyn std::error::Error>> {
    let mut keyword: Option<String> = None;
    let mut domain: Option<String> = None;
    let mut tag: Option<String> = None;
    let mut status: Option<String> = None;

    // Walk through args, consuming flags and their values
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--domain" => {
                i += 1;
                domain = Some(
                    args.get(i)
                        .ok_or("--domain requires a value")?
                        .clone(),
                );
            }
            "--tag" => {
                i += 1;
                tag = Some(
                    args.get(i)
                        .ok_or("--tag requires a value")?
                        .clone(),
                );
            }
            "--status" => {
                i += 1;
                status = Some(
                    args.get(i)
                        .ok_or("--status requires a value")?
                        .clone(),
                );
            }
            other => {
                // Not a flag - treat as keyword
                // If multiple non-flag words, join them
                if let Some(ref mut kw) = keyword {
                    kw.push(' ');
                    kw.push_str(other);
                } else {
                    keyword = Some(other.to_string());
                }
            }
        }
        i += 1;
    }

    Ok(SearchQuery {
        keyword,
        domain,
        tag,
        status,
    })
}

/// Check if a feature matches the search query
///
/// All provided filters must match (AND logic)
/// Keyword search is case-insensitive across multiple fields
fn matches_query(feature: &Feature, query: &SearchQuery) -> bool {
    // Check keyword (if provided) - search across multiple fields
    if let Some(ref kw) = query.keyword {
        let kw_lower = kw.to_lowercase();
        let matches_keyword = feature.id.to_lowercase().contains(&kw_lower)
            || feature.name.to_lowercase().contains(&kw_lower)
            || feature.domain.to_lowercase().contains(&kw_lower)
            || feature.description.to_lowercase().contains(&kw_lower)
            || feature
                .context
                .as_ref()
                .map(|c| c.to_lowercase().contains(&kw_lower))
                .unwrap_or(false)
            || feature
                .tags
                .iter()
                .any(|t| t.to_lowercase().contains(&kw_lower));

        if !matches_keyword {
            return false;
        }
    }

    // Check domain filter
    if let Some(ref d) = query.domain {
        if feature.domain.to_lowercase() != d.to_lowercase() {
            return false;
        }
    }

    // Check tag filter
    if let Some(ref t) = query.tag {
        let t_lower = t.to_lowercase();
        if !feature.tags.iter().any(|tag| tag.to_lowercase() == t_lower) {
            return false;
        }
    }

    // Check status filter
    if let Some(ref s) = query.status {
        let status_str = format!("{:?}", feature.status); // Debug format gives variant name
        if status_str.to_lowercase() != s.to_lowercase() {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Feature, FeatureStatus};

    fn make_feature(id: &str, name: &str, domain: &str, desc: &str) -> Feature {
        let mut f = Feature::new(
            id.to_string(),
            name.to_string(),
            domain.to_string(),
            desc.to_string(),
        );
        f.tags = vec!["backend".to_string()];
        f
    }

    #[test]
    fn test_keyword_matches_id() {
        let f = make_feature("auth-login", "Login", "security", "Login page");
        let q = SearchQuery {
            keyword: Some("auth".to_string()),
            domain: None,
            tag: None,
            status: None,
        };
        assert!(matches_query(&f, &q));
    }

    #[test]
    fn test_keyword_matches_description() {
        let f = make_feature("feat1", "Feature", "cli", "Handles user authentication");
        let q = SearchQuery {
            keyword: Some("authentication".to_string()),
            domain: None,
            tag: None,
            status: None,
        };
        assert!(matches_query(&f, &q));
    }

    #[test]
    fn test_keyword_no_match() {
        let f = make_feature("feat1", "Feature", "cli", "Does something");
        let q = SearchQuery {
            keyword: Some("auth".to_string()),
            domain: None,
            tag: None,
            status: None,
        };
        assert!(!matches_query(&f, &q));
    }

    #[test]
    fn test_domain_filter() {
        let f = make_feature("feat1", "Feature", "security", "Something");
        let q = SearchQuery {
            keyword: None,
            domain: Some("security".to_string()),
            tag: None,
            status: None,
        };
        assert!(matches_query(&f, &q));
    }

    #[test]
    fn test_combined_filters() {
        let mut f = make_feature("auth", "Auth", "security", "Login system");
        f.status = FeatureStatus::InProgress;

        let q = SearchQuery {
            keyword: Some("auth".to_string()),
            domain: Some("security".to_string()),
            tag: None,
            status: Some("InProgress".to_string()),
        };
        assert!(matches_query(&f, &q));
    }

    #[test]
    fn test_case_insensitive() {
        let f = make_feature("AUTH", "Auth System", "Security", "LOGIN");
        let q = SearchQuery {
            keyword: Some("auth".to_string()),
            domain: Some("security".to_string()),
            tag: None,
            status: None,
        };
        assert!(matches_query(&f, &q));
    }

    #[test]
    fn test_tag_filter() {
        let f = make_feature("feat1", "Feature", "cli", "Something");
        let q = SearchQuery {
            keyword: None,
            domain: None,
            tag: Some("backend".to_string()),
            status: None,
        };
        assert!(matches_query(&f, &q));
    }
}
