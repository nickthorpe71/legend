/// Entity extraction from text (code-aware + plain identifiers).

use std::collections::HashMap;

pub struct ExtractedEntity {
    pub label: String,
    pub kind: String,
    /// Relationship context: "defines", "uses", "implements", "mentions"
    pub context: String,
}

/// Extract entities from text — multi-pass: code keywords, paths, actions, environments, and identifiers.
pub fn extract_entities(text: &str) -> Vec<ExtractedEntity> {
    let mut entities = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();

        // 1. Path patterns
        for token in trimmed.split_whitespace() {
            let clean_token = token.trim_matches(|c: char| matches!(c, '`' | '"' | '\'' | ',' | '.' | ')' | ']' | ';'));
            if (clean_token.contains('/') || clean_token.contains('\\')) && clean_token.contains('.') && clean_token.len() > 4 {
                entities.push(ExtractedEntity {
                    label: clean_token.to_string(),
                    kind: "FilePath".to_string(),
                    context: "mentions".to_string(),
                });
            }
        }

        // 2. Action patterns (Verbs at the start of lines or sentences)
        let actions = [
            ("fixed", "Action"), ("fixing", "Action"), ("refactored", "Action"), 
            ("refactoring", "Action"), ("implemented", "Action"), ("implementing", "Action"),
            ("added", "Action"), ("adding", "Action"), ("removed", "Action"), 
            ("removing", "Action"), ("tested", "Action"), ("testing", "Action"),
            ("debugged", "Action"), ("debugging", "Action"), ("optimized", "Action"),
        ];
        for (verb, kind) in actions {
            if lower.starts_with(verb) || lower.contains(&format!(". {}", verb)) {
                entities.push(ExtractedEntity {
                    label: verb.to_string(),
                    kind: kind.to_string(),
                    context: "performs".to_string(),
                });
            }
        }

        // 3. Environment patterns
        let envs = [
            "wsl", "docker", "production", "staging", "ubuntu", "linux", "windows", 
            "macos", "s3", "github", "aws", "localhost", "browser", "cli",
        ];
        for env in envs {
            if lower.contains(env) {
                // Find original casing if possible, or use the env string
                entities.push(ExtractedEntity {
                    label: env.to_string(),
                    kind: "Environment".to_string(),
                    context: "mentions".to_string(),
                });
            }
        }

        // 4. Code patterns (Multi-language)
        // Rust/Python/JS/Java/C++ keywords
        let keywords = [
            ("fn ", "Function", "defines"), ("def ", "Function", "defines"), 
            ("function ", "Function", "defines"), ("struct ", "Struct", "defines"),
            ("class ", "Class", "defines"), ("interface ", "Interface", "defines"),
            ("trait ", "Trait", "defines"), ("impl ", "Impl", "implements"),
            ("mod ", "Module", "defines"), ("module ", "Module", "defines"),
            ("use ", "Import", "uses"), ("import ", "Import", "uses"),
            ("package ", "Package", "defines"), ("export ", "Export", "defines"),
            ("let ", "Symbol", "defines"), ("var ", "Symbol", "defines"),
            ("const ", "Symbol", "defines"),
        ];
        for (kw, kind, ctx) in keywords {
            try_extract(trimmed, kw, kind, ctx, &mut entities);
        }

        // 5. Language-agnostic patterns (assignment, decoration)
        // Name = ...
        if let Some(pos) = trimmed.find(" = ") {
            let lhs = trimmed[..pos].trim();
            if is_identifier(lhs) && lhs.len() > 3 && !is_stopword(lhs) {
                entities.push(ExtractedEntity {
                    label: lhs.to_string(),
                    kind: "Symbol".to_string(),
                    context: "defines".to_string(),
                });
            }
        }
        // @Decorator
        if let Some(pos) = trimmed.find('@') {
            let rest = &trimmed[pos+1..];
            let name: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            if name.len() > 2 && !is_stopword(&name) {
                entities.push(ExtractedEntity {
                    label: name,
                    kind: "Decorator".to_string(),
                    context: "mentions".to_string(),
                });
            }
        }
    }

    // 6. Plain identifiers for remaining text
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
    let mut seen = std::collections::HashSet::new();
    entities.retain(|e| seen.insert(e.label.clone()));
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

    // Skip common modifiers to find the actual identifier
    let modifiers = ["const ", "static ", "async ", "pub ", "public ", "private ", "protected ", "final ", "readonly "];
    let mut current = rest.trim_start();
    let mut changed = true;
    while changed {
        changed = false;
        for m in modifiers {
            if current.starts_with(m) {
                current = current[m.len()..].trim_start();
                changed = true;
            }
        }
    }

    let name: String = current.chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
        .collect();

    if !name.is_empty() && !is_stopword(&name) { Some(name) } else { None }
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
        // Article, pronouns, prepositions
        "the" | "and" | "for" | "with" | "this" | "that" | "from" | "when" | "then" | "into"
            | "your" | "you" | "our" | "are" | "was" | "were" | "been" | "have" | "has"
            | "had" | "will" | "should" | "would" | "could" | "can" | "may" | "might"
            | "not" | "but" | "its" | "it" | "they" | "them" | "their" | "she" | "her"
            | "his" | "him" | "who" | "what" | "which" | "how" | "why" | "where"
        // Common generic verbs & adverbs
            | "also" | "just" | "some" | "each" | "many" | "most" | "more" | "very"
            | "much" | "only" | "other" | "about" | "after" | "before" | "between"
            | "through" | "during" | "without" | "within" | "using" | "used" | "does"
            | "did" | "done" | "being" | "need" | "needs" | "make" | "made" | "like"
            | "get" | "got" | "set" | "let" | "see" | "take" | "took" | "run" | "ran"
        // Positional / temporal
            | "new" | "old" | "first" | "last" | "next" | "still" | "already" | "now"
            | "here" | "there" | "all" | "any" | "both" | "every" | "same" | "such"
        // Common output noise from build/status messages
            | "files" | "zero" | "warnings" | "total" | "passed"
            | "running" | "test" | "tests" | "note"
            | "line" | "lines" | "code" | "change" 
            | "working" | "build" | "checked" | "checking" | "check"
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
    } else if label.contains('_') || (label.chars().any(|c| c.is_lowercase()) && label.chars().any(|c| c.is_uppercase())) {
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
        let ids = extract_identifiers("zero warnings all files passed total tests running");
        // All these are stopwords now
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
    fn test_extract_actions() {
        let entities = extract_entities("Fixed the bug in storage. Refactored the TUI.");
        let labels: Vec<&str> = entities.iter().map(|e| e.label.as_str()).collect();
        assert!(labels.contains(&"fixed"));
        assert!(labels.contains(&"refactored"));
        assert_eq!(entities.iter().find(|e| e.label == "fixed").unwrap().kind, "Action");
    }

    #[test]
    fn test_extract_environments() {
        let entities = extract_entities("This issue only happens on wsl and docker.");
        let labels: Vec<&str> = entities.iter().map(|e| e.label.as_str()).collect();
        assert!(labels.contains(&"wsl"));
        assert!(labels.contains(&"docker"));
        assert_eq!(entities.iter().find(|e| e.label == "wsl").unwrap().kind, "Environment");
    }

    #[test]
    fn test_extract_assignment_pattern() {
        let entities = extract_entities("my_config_value = 42");
        let labels: Vec<&str> = entities.iter().map(|e| e.label.as_str()).collect();
        assert!(labels.contains(&"my_config_value"));
        assert_eq!(entities.iter().find(|e| e.label == "my_config_value").unwrap().kind, "Symbol");
    }

    #[test]
    fn test_extract_decorator_pattern() {
        let entities = extract_entities("@Component class MyUI {}");
        let labels: Vec<&str> = entities.iter().map(|e| e.label.as_str()).collect();
        assert!(labels.contains(&"Component"));
        assert_eq!(entities.iter().find(|e| e.label == "Component").unwrap().kind, "Decorator");
    }

    #[test]
    fn test_multi_language_keywords() {
        let entities = extract_entities("interface IService {}; package com.legend; export const X = 1;");
        let labels: Vec<&str> = entities.iter().map(|e| e.label.as_str()).collect();
        assert!(labels.contains(&"IService"));
        assert!(labels.contains(&"com.legend"));
        assert!(labels.contains(&"X"));
    }
}
