use super::event_log::*;
use super::helpers::truncate_text;
use crate::cli::{parse_args, CommandDef};
use crate::memory::MemoryContext;

#[derive(Default)]
struct QueryOptions {
    query: String,
    show_reasons: bool,
}

fn parse_query_args(args: &[String], def: &CommandDef) -> Result<QueryOptions, Box<dyn std::error::Error>> {
    let parsed = parse_args(args, def);

    if parsed.positional.is_empty() {
        return Err("Provide a query string".into());
    }

    Ok(QueryOptions {
        query: parsed.positional.join(" "),
        show_reasons: parsed.has("reasons"),
    })
}

pub(super) fn handle_query(args: &[String], def: &CommandDef) -> Result<(), Box<dyn std::error::Error>> {
    let opts = parse_query_args(args, def)?;

    let mut memory = crate::memory::load_or_default()?;
    let context = crate::memory::retrieve_context(&mut memory.brain, opts.query.trim());
    crate::memory::save(&memory)?;

    let primed_count = context
        .long_term
        .iter()
        .filter(|n| n.edge_type.is_some())
        .count();

    let event_data = EventData::Query(QueryEventData {
        matches: context
            .short_term
            .iter()
            .take(5)
            .map(|m| MatchedEntry {
                id: m.id,
                similarity: m.similarity,
                text_preview: truncate_text(&m.text, 80),
            })
            .collect(),
        graph_nodes: context
            .long_term
            .iter()
            .take(8)
            .map(|n| GraphHit {
                id: n.id,
                label: n.label.clone(),
                kind: n.kind.clone(),
                weight: n.weight,
            })
            .collect(),
        primed_count,
    });
    log_event_rich("query", opts.query.trim(), Some(event_data));

    if opts.show_reasons {
        print_query_with_reasons(&context, primed_count);
    } else {
        print_context(context);
    }
    Ok(())
}

fn print_query_with_reasons(context: &MemoryContext, primed_count: usize) {
    let working_memory_with_reasons: Vec<serde_json::Value> = context
        .working_memory
        .iter()
        .map(|m| {
            serde_json::json!({
                "id": m.id,
                "text": m.text,
                "similarity": (m.similarity * 1000.0).round() / 1000.0,
                "reason": "matched in working memory (L1), rehearsal incremented",
            })
        })
        .collect();

    let short_term_with_reasons: Vec<serde_json::Value> = context
        .short_term
        .iter()
        .map(|m| {
            let reason = if m.similarity >= 0.8 {
                "high semantic similarity to query"
            } else if m.similarity >= 0.5 {
                "moderate semantic similarity to query"
            } else if m.similarity >= 0.2 {
                "weak semantic similarity, may share related terms"
            } else {
                "low similarity, included for coverage"
            };
            serde_json::json!({
                "id": m.id,
                "text": m.text,
                "similarity": (m.similarity * 1000.0).round() / 1000.0,
                "reason": reason,
            })
        })
        .collect();

    let long_term_with_reasons: Vec<serde_json::Value> = context
        .long_term
        .iter()
        .map(|n| {
            let reason = if n.edge_type.is_some() {
                format!(
                    "reached via {} edge from related entity",
                    n.edge_type.as_ref().unwrap()
                )
            } else {
                "direct entity match from query".to_string()
            };
            serde_json::json!({
                "id": n.id,
                "label": n.label,
                "kind": n.kind,
                "weight": (n.weight * 1000.0).round() / 1000.0,
                "reason": reason,
            })
        })
        .collect();

    let result = serde_json::json!({
        "working_memory": working_memory_with_reasons,
        "short_term": short_term_with_reasons,
        "long_term": long_term_with_reasons,
        "primed_via_edges": primed_count,
        "note": "Top result auto-reinforced (+3% salience boost)"
    });

    let json = serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string());
    println!("{}", json);
}

fn print_context(context: MemoryContext) {
    let working_memory: Vec<&str> = context.working_memory.iter().map(|m| m.text.as_str()).collect();
    let memories: Vec<&str> = context.short_term.iter().map(|m| m.text.as_str()).collect();
    let related_topics: Vec<&str> = context.long_term.iter().map(|n| n.label.as_str()).collect();

    let result = serde_json::json!({
        "working_memory": working_memory,
        "memories": memories,
        "related_topics": related_topics,
    });
    let json = serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string());
    println!("{}", json);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::FlagDef;

    static TEST_QUERY_DEF: CommandDef = CommandDef {
        name: "query",
        about: "Query memory",
        usage: "legend memory query [--reasons] <text>",
        flags: &[
            FlagDef { long: "--reasons", short: Some('r'), about: "Include retrieval reasoning", takes_value: false },
        ],
        positionals: &[],
        children: &[],
    };

    #[test]
    fn test_parse_query_args_simple() {
        let args = vec!["memory".to_string(), "system".to_string()];
        let opts = parse_query_args(&args, &TEST_QUERY_DEF).unwrap();
        assert_eq!(opts.query, "memory system");
        assert!(!opts.show_reasons);
    }

    #[test]
    fn test_parse_query_args_reasons_flag() {
        let args = vec!["--reasons".to_string(), "test".to_string()];
        let opts = parse_query_args(&args, &TEST_QUERY_DEF).unwrap();
        assert!(opts.show_reasons);
        assert_eq!(opts.query, "test");
    }

    #[test]
    fn test_parse_query_args_short_reasons() {
        let args = vec!["-r".to_string(), "query".to_string()];
        let opts = parse_query_args(&args, &TEST_QUERY_DEF).unwrap();
        assert!(opts.show_reasons);
    }

    #[test]
    fn test_parse_query_args_empty_rejects() {
        let args: Vec<String> = vec![];
        assert!(parse_query_args(&args, &TEST_QUERY_DEF).is_err());
    }
}
