/// Entity extraction from text (code-aware + plain identifiers).

use std::collections::HashMap;

pub struct ExtractedEntity {
    pub label: String,
    pub kind: String,
    /// Relationship context: "defines", "uses", "implements", "mentions"
    pub context: String,
}

/// Extract entities from text — code keywords first, then plain identifiers.
pub fn extract_entities(text: &str) -> Vec<ExtractedEntity> {
    let mut entities = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();

        // Rust patterns
        try_extract(trimmed, "fn ", "Function", "defines", &mut entities);
        try_extract(trimmed, "struct ", "Struct", "defines", &mut entities);
        try_extract(trimmed, "enum ", "Enum", "defines", &mut entities);
        try_extract(trimmed, "trait ", "Trait", "defines", &mut entities);
        try_extract(trimmed, "impl ", "Impl", "implements", &mut entities);
        try_extract(trimmed, "mod ", "Module", "defines", &mut entities);
        try_extract(trimmed, "use ", "Import", "uses", &mut entities);
        // Python/JS patterns
        try_extract(trimmed, "def ", "Function", "defines", &mut entities);
        try_extract(trimmed, "class ", "Class", "defines", &mut entities);
        try_extract(trimmed, "function ", "Function", "defines", &mut entities);
    }

    // Plain identifiers for non-code text
    for label in extract_identifiers(text) {
        if !entities.iter().any(|e| e.label == label) {
            entities.push(ExtractedEntity {
                kind: infer_kind(&label),
                label,
                context: "mentions".to_string(),
            });
        }
    }

    // Deduplicate by label
    let mut seen = HashMap::new();
    entities.retain(|e| seen.insert(e.label.clone(), ()).is_none());
    entities
}

/// Try to extract an identifier after a code keyword (e.g. "fn ", "struct ").
fn try_extract(line: &str, keyword: &str, kind: &str, context: &str, out: &mut Vec<ExtractedEntity>) {
    if let Some(name) = extract_after_keyword(line, keyword) {
        out.push(ExtractedEntity {
            label: name,
            kind: kind.to_string(),
            context: context.to_string(),
        });
    }
}

/// Parse the identifier name immediately following a keyword like "fn " or "struct ".
fn extract_after_keyword(line: &str, keyword: &str) -> Option<String> {
    let rest = line.strip_prefix(keyword)
        .or_else(|| line.find(keyword).map(|pos| &line[pos + keyword.len()..]))?;

    let name: String = rest.trim().chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();

    if name.len() > 1 && !is_stopword(&name) { Some(name) } else { None }
}

/// Scan text for standalone identifiers (alphanumeric + underscore tokens).
fn extract_identifiers(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            current.push(ch);
        } else if !current.is_empty() {
            if is_identifier(&current) {
                tokens.push(current.clone());
            }
            current.clear();
        }
    }
    if !current.is_empty() && is_identifier(&current) {
        tokens.push(current);
    }

    let mut unique = HashMap::new();
    for token in tokens.into_iter().filter(|t| {
        t.len() > 2
            && !is_stopword(t)
            && !t.chars().all(|c| c.is_ascii_digit()) // filter pure numbers
    }) {
        unique.entry(token).or_insert(());
    }
    unique.into_keys().collect()
}

/// True if the token looks like a valid identifier (starts with letter or underscore).
fn is_identifier(token: &str) -> bool {
    let mut chars = token.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Common English words and project-specific terms to exclude from entity extraction.
pub fn is_stopword(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        // Articles, pronouns, prepositions
        "the" | "and" | "for" | "with" | "this" | "that" | "from" | "when" | "then" | "into"
            | "your" | "you" | "our" | "are" | "was" | "were" | "been" | "have" | "has"
            | "had" | "will" | "should" | "would" | "could" | "can" | "may" | "might"
            | "not" | "but" | "its" | "it" | "they" | "them" | "their" | "she" | "her"
            | "his" | "him" | "who" | "what" | "which" | "how" | "why" | "where"
        // Common verbs & adverbs
            | "also" | "just" | "some" | "each" | "many" | "most" | "more" | "very"
            | "much" | "only" | "other" | "about" | "after" | "before" | "between"
            | "through" | "during" | "without" | "within" | "using" | "used" | "does"
            | "did" | "done" | "being" | "need" | "needs" | "make" | "made" | "like"
            | "get" | "got" | "set" | "let" | "see" | "take" | "took" | "run" | "ran"
        // Positional / temporal
            | "new" | "old" | "first" | "last" | "next" | "still" | "already" | "now"
            | "here" | "there" | "all" | "any" | "both" | "every" | "same" | "such"
        // Common output noise from build/status messages
            | "added" | "files" | "zero" | "warnings" | "total" | "passed" | "failed"
            | "running" | "test" | "tests" | "error" | "errors" | "warning" | "note"
            | "line" | "lines" | "code" | "changes" | "change" | "update" | "updated"
        // Project-specific
            | "memory" | "legend" | "term"
        // Generic infrastructure / hook noise terms that pollute L3
            | "tool" | "success" | "status" | "state" | "file" | "text"
            | "agent" | "turn" | "bash" | "session" | "goal" | "tick"
            | "context" | "graph" | "nodes" | "entry" | "data" | "query"
            | "output" | "start" | "input" | "result" | "results" | "path"
            | "value" | "values" | "type" | "types" | "item" | "items"
            | "list" | "map" | "key" | "keys" | "true" | "false" | "none"
            | "null" | "empty" | "count" | "size" | "len" | "num"
    )
}

/// Classify an identifier as Type (uppercase), Symbol (has underscore), or Term.
fn infer_kind(label: &str) -> String {
    if label.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
        "Type".to_string()
    } else if label.contains('_') {
        "Symbol".to_string()
    } else {
        "Term".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_entities_rust_code() {
        let entities = extract_entities("fn handle_memory() { struct MemoryState {} }");
        let labels: Vec<&str> = entities.iter().map(|e| e.label.as_str()).collect();
        assert!(labels.contains(&"handle_memory"), "got: {:?}", labels);
        assert!(labels.contains(&"MemoryState"), "got: {:?}", labels);
    }

    #[test]
    fn test_extract_entities_python() {
        let entities = extract_entities("def process_data(): class DataProcessor:");
        let labels: Vec<&str> = entities.iter().map(|e| e.label.as_str()).collect();
        assert!(labels.contains(&"process_data"), "got: {:?}", labels);
        assert!(labels.contains(&"DataProcessor"), "got: {:?}", labels);
    }

    #[test]
    fn test_extract_identifiers_filters_stopwords() {
        let ids = extract_identifiers("the memory system for this project with legend");
        assert!(!ids.contains(&"the".to_string()));
        assert!(!ids.contains(&"for".to_string()));
    }

    #[test]
    fn test_expanded_stopwords_filter_noise() {
        let ids = extract_identifiers("Added zero warnings all files passed total tests running");
        // All these are stopwords now
        assert!(!ids.contains(&"Added".to_string()));
        assert!(!ids.contains(&"zero".to_string()));
        assert!(!ids.contains(&"warnings".to_string()));
        assert!(!ids.contains(&"files".to_string()));
        assert!(!ids.contains(&"total".to_string()));
    }

    #[test]
    fn test_numeric_tokens_filtered() {
        let ids = extract_identifiers("version 123 has 456 items and config_v2");
        assert!(!ids.contains(&"123".to_string()));
        assert!(!ids.contains(&"456".to_string()));
        assert!(ids.contains(&"config_v2".to_string()));
    }

    #[test]
    fn test_infer_kind() {
        assert_eq!(infer_kind("MemoryState"), "Type");
        assert_eq!(infer_kind("handle_memory"), "Symbol");
        assert_eq!(infer_kind("config"), "Term");
    }
}
