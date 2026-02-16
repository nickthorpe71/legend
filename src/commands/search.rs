use crate::storage;
use crate::types::Feature;

/// Search features by keyword, domain, tag, or status. Output JSON to stdout.
pub fn handle_search(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.is_empty() {
        return Err("Usage: legend search <query> [--domain <d>] [--tag <t>] [--status <s>]".into());
    }

    let query = parse_args(args)?;
    let state = storage::load_state()?;

    let results: Vec<&Feature> = state.features.iter().filter(|f| matches_query(f, &query)).collect();

    if results.is_empty() {
        println!("[]");
        eprintln!("No features matched the search.");
        return Ok(());
    }

    let json = serde_json::to_string_pretty(&results)
        .map_err(|e| format!("Failed to serialize results: {}", e))?;
    println!("{}", json);
    eprintln!("Found {} matching feature(s).", results.len());

    Ok(())
}

/// Parsed search filters — all provided fields use AND logic.
struct SearchQuery {
    keyword: Option<String>,
    domain: Option<String>,
    tag: Option<String>,
    status: Option<String>,
}

fn parse_args(args: &[String]) -> Result<SearchQuery, Box<dyn std::error::Error>> {
    let mut keyword: Option<String> = None;
    let mut domain: Option<String> = None;
    let mut tag: Option<String> = None;
    let mut status: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--domain" => { i += 1; domain = Some(args.get(i).ok_or("--domain requires a value")?.clone()); }
            "--tag" => { i += 1; tag = Some(args.get(i).ok_or("--tag requires a value")?.clone()); }
            "--status" => { i += 1; status = Some(args.get(i).ok_or("--status requires a value")?.clone()); }
            other => {
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

    Ok(SearchQuery { keyword, domain, tag, status })
}

/// All provided filters must match (AND logic). Keyword is case-insensitive.
fn matches_query(feature: &Feature, query: &SearchQuery) -> bool {
    if let Some(ref kw) = query.keyword {
        let kw_lower = kw.to_lowercase();
        let matches_keyword = feature.id.to_lowercase().contains(&kw_lower)
            || feature.name.to_lowercase().contains(&kw_lower)
            || feature.domain.to_lowercase().contains(&kw_lower)
            || feature.description.to_lowercase().contains(&kw_lower)
            || feature.context.as_ref().map(|c| c.to_lowercase().contains(&kw_lower)).unwrap_or(false)
            || feature.tags.iter().any(|t| t.to_lowercase().contains(&kw_lower));
        if !matches_keyword { return false; }
    }

    if let Some(ref d) = query.domain {
        if feature.domain.to_lowercase() != d.to_lowercase() { return false; }
    }

    if let Some(ref t) = query.tag {
        let t_lower = t.to_lowercase();
        if !feature.tags.iter().any(|tag| tag.to_lowercase() == t_lower) { return false; }
    }

    if let Some(ref s) = query.status {
        let status_str = format!("{:?}", feature.status);
        if status_str.to_lowercase() != s.to_lowercase() { return false; }
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
