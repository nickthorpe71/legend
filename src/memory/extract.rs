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
            let clean_token = token.trim_matches(|c: char| {
                matches!(c, '`' | '"' | '\'' | ',' | '.' | ')' | ']' | ';')
            });
            if (clean_token.contains('/') || clean_token.contains('\\'))
                && clean_token.contains('.')
                && clean_token.len() > 4
            {
                entities.push(ExtractedEntity {
                    label: clean_token.to_string(),
                    kind: "FilePath".to_string(),
                    context: "mentions".to_string(),
                });
            }
        }

        // 2. Action patterns (Verbs at the start of lines or sentences)
        let actions = [
            ("fixed", "Action"),
            ("fixing", "Action"),
            ("refactored", "Action"),
            ("refactoring", "Action"),
            ("implemented", "Action"),
            ("implementing", "Action"),
            ("added", "Action"),
            ("adding", "Action"),
            ("removed", "Action"),
            ("removing", "Action"),
            ("tested", "Action"),
            ("testing", "Action"),
            ("debugged", "Action"),
            ("debugging", "Action"),
            ("optimized", "Action"),
            ("migrated", "Action"),
            ("migrating", "Action"),
            ("deployed", "Action"),
            ("deploying", "Action"),
            ("validated", "Action"),
            ("validating", "Action"),
            ("investigated", "Action"),
            ("investigating", "Action"),
            ("documented", "Action"),
            ("documenting", "Action"),
            ("reverted", "Action"),
            ("reverting", "Action"),
            ("rolled back", "Action"),
            ("rollback", "Action"),
        ];
        for (verb, kind) in actions {
            if sentence_contains_phrase(&lower, verb) {
                entities.push(ExtractedEntity {
                    label: verb.to_string(),
                    kind: kind.to_string(),
                    context: "performs".to_string(),
                });
            }
        }

        // 3. Environment patterns
        let envs = [
            "wsl",
            "docker",
            "production",
            "staging",
            "ubuntu",
            "linux",
            "windows",
            "macos",
            "s3",
            "github",
            "aws",
            "localhost",
            "browser",
            "cli",
            "kubernetes",
            "k8s",
            "gcp",
            "azure",
            "devcontainer",
            "vm",
            "ci",
            "cd",
            "github actions",
        ];
        for env in envs {
            if sentence_contains_phrase(&lower, env) {
                // Find original casing if possible, or use the env string
                entities.push(ExtractedEntity {
                    label: env.to_string(),
                    kind: "Environment".to_string(),
                    context: "mentions".to_string(),
                });
            }
        }

        // 3b. High-signal technology/tool patterns (constrained dictionary)
        let tools = [
            "postgres",
            "postgresql",
            "redis",
            "kafka",
            "grpc",
            "graphql",
            "react",
            "nextjs",
            "tailwind",
            "vite",
            "webpack",
            "jest",
            "pytest",
            "tokio",
            "actix",
            "axum",
            "fastapi",
            "django",
            "flask",
            "terraform",
            "ansible",
            "prometheus",
            "grafana",
            "nginx",
        ];
        for tool in tools {
            if sentence_contains_phrase(&lower, tool) {
                entities.push(ExtractedEntity {
                    label: tool.to_string(),
                    kind: "Tool".to_string(),
                    context: "mentions".to_string(),
                });
            }
        }

        // 4. Code patterns (Multi-language)
        // Rust/Python/JS/Java/C++ keywords
        let keywords = [
            ("fn ", "Function", "defines"),
            ("def ", "Function", "defines"),
            ("function ", "Function", "defines"),
            ("struct ", "Struct", "defines"),
            ("class ", "Class", "defines"),
            ("interface ", "Interface", "defines"),
            ("trait ", "Trait", "defines"),
            ("impl ", "Impl", "implements"),
            ("mod ", "Module", "defines"),
            ("module ", "Module", "defines"),
            ("use ", "Import", "uses"),
            ("import ", "Import", "uses"),
            ("package ", "Package", "defines"),
            ("export ", "Export", "defines"),
            ("let ", "Symbol", "defines"),
            ("var ", "Symbol", "defines"),
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
            let rest = &trimmed[pos + 1..];
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
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
fn try_extract(
    line: &str,
    keyword: &str,
    kind: &str,
    context: &str,
    out: &mut Vec<ExtractedEntity>,
) {
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
    let rest = line
        .strip_prefix(keyword)
        .or_else(|| line.find(keyword).map(|pos| &line[pos + keyword.len()..]))?;

    // Skip common modifiers to find the actual identifier
    let modifiers = [
        "const ",
        "static ",
        "async ",
        "pub ",
        "public ",
        "private ",
        "protected ",
        "final ",
        "readonly ",
    ];
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

    let name: String = current
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
        .collect();

    if !name.is_empty() && !is_stopword(&name) {
        Some(name)
    } else {
        None
    }
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
    for token in tokens {
        for variant in expand_identifier_variants(&token) {
            if variant.len() > 2
                && !is_stopword(&variant)
                && !variant.chars().all(|c| c.is_ascii_digit())
            {
                unique.entry(variant).or_insert(());
            }
        }
    }
    unique.into_keys().collect()
}

/// Normalize identifier tokens with constrained variants:
/// original token + snake/camel components + simple singular form.
fn expand_identifier_variants(token: &str) -> Vec<String> {
    let mut out: HashMap<String, ()> = HashMap::new();
    out.insert(token.to_string(), ());

    for part in split_identifier_parts(token) {
        out.insert(part.clone(), ());
        let lower_part = part.to_ascii_lowercase();
        out.insert(lower_part.clone(), ());
        if let Some(singular) = singularize(&lower_part) {
            out.insert(singular, ());
        }
    }

    let lower = token.to_ascii_lowercase();
    out.insert(lower.clone(), ());
    if let Some(singular) = singularize(&lower) {
        out.insert(singular, ());
    }

    out.into_keys().collect()
}

fn split_identifier_parts(token: &str) -> Vec<String> {
    let mut parts = Vec::new();

    for raw in token.split(['_', '-']) {
        if raw.is_empty() {
            continue;
        }
        parts.push(raw.to_string());
        parts.extend(split_camel_case(raw));
    }

    parts
}

fn split_camel_case(input: &str) -> Vec<String> {
    if input.is_empty() {
        return Vec::new();
    }

    let mut parts = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = input.chars().collect();

    for (idx, &ch) in chars.iter().enumerate() {
        let prev = if idx > 0 { Some(chars[idx - 1]) } else { None };
        let next = chars.get(idx + 1).copied();
        let is_boundary = match (prev, next) {
            (Some(p), Some(n)) => {
                (p.is_ascii_lowercase() && ch.is_ascii_uppercase())
                    || (p.is_ascii_alphabetic() && ch.is_ascii_digit())
                    || (p.is_ascii_digit() && ch.is_ascii_alphabetic())
                    || (p.is_ascii_uppercase() && ch.is_ascii_uppercase() && n.is_ascii_lowercase())
            }
            (Some(p), None) => {
                (p.is_ascii_lowercase() && ch.is_ascii_uppercase())
                    || (p.is_ascii_alphabetic() && ch.is_ascii_digit())
                    || (p.is_ascii_digit() && ch.is_ascii_alphabetic())
            }
            _ => false,
        };

        if is_boundary && !current.is_empty() {
            parts.push(current.clone());
            current.clear();
        }
        current.push(ch);
    }

    if !current.is_empty() {
        parts.push(current);
    }

    parts
}

fn singularize(token: &str) -> Option<String> {
    if token.len() <= 3 {
        return None;
    }

    if token.ends_with("ies") && token.len() > 4 {
        return Some(format!("{}y", &token[..token.len() - 3]));
    }
    if token.ends_with("es") && token.len() > 4 {
        let stem = &token[..token.len() - 2];
        if !stem.ends_with('s') {
            return Some(stem.to_string());
        }
    }
    if token.ends_with('s') && !token.ends_with("ss") {
        return Some(token[..token.len() - 1].to_string());
    }

    None
}

/// Phrase match with lightweight sentence/word boundaries.
fn sentence_contains_phrase(lower_text: &str, phrase: &str) -> bool {
    if lower_text.starts_with(phrase) {
        return true;
    }

    let patterns = [
        format!(". {}", phrase),
        format!("! {}", phrase),
        format!("? {}", phrase),
        format!(", {}", phrase),
        format!("; {}", phrase),
        format!(": {}", phrase),
        format!(" {}", phrase),
    ];

    patterns.iter().any(|p| lower_text.contains(p))
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
    if label
        .chars()
        .next()
        .map(|c| c.is_uppercase())
        .unwrap_or(false)
    {
        "Type".to_string()
    } else if label.contains('_')
        || (label.chars().any(|c| c.is_lowercase()) && label.chars().any(|c| c.is_uppercase()))
    {
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
        assert_eq!(
            entities.iter().find(|e| e.label == "fixed").unwrap().kind,
            "Action"
        );
    }

    #[test]
    fn test_extract_environments() {
        let entities = extract_entities("This issue only happens on wsl and docker.");
        let labels: Vec<&str> = entities.iter().map(|e| e.label.as_str()).collect();
        assert!(labels.contains(&"wsl"));
        assert!(labels.contains(&"docker"));
        assert_eq!(
            entities.iter().find(|e| e.label == "wsl").unwrap().kind,
            "Environment"
        );
    }

    #[test]
    fn test_extract_expanded_actions_and_envs() {
        let entities = extract_entities(
            "Investigating deploy failures in kubernetes. We rolled back in production.",
        );
        let labels: Vec<&str> = entities.iter().map(|e| e.label.as_str()).collect();
        assert!(labels.contains(&"investigating"), "got: {:?}", labels);
        assert!(labels.contains(&"rolled back"), "got: {:?}", labels);
        assert!(labels.contains(&"kubernetes"), "got: {:?}", labels);
        assert!(labels.contains(&"production"), "got: {:?}", labels);
    }

    #[test]
    fn test_extract_tools_dictionary() {
        let entities = extract_entities(
            "We moved services to graphql + redis and added prometheus dashboards.",
        );
        let labels: Vec<&str> = entities.iter().map(|e| e.label.as_str()).collect();
        assert!(labels.contains(&"graphql"), "got: {:?}", labels);
        assert!(labels.contains(&"redis"), "got: {:?}", labels);
        assert!(labels.contains(&"prometheus"), "got: {:?}", labels);
        assert_eq!(
            entities.iter().find(|e| e.label == "graphql").unwrap().kind,
            "Tool"
        );
    }

    #[test]
    fn test_extract_assignment_pattern() {
        let entities = extract_entities("my_config_value = 42");
        let labels: Vec<&str> = entities.iter().map(|e| e.label.as_str()).collect();
        assert!(labels.contains(&"my_config_value"));
        assert_eq!(
            entities
                .iter()
                .find(|e| e.label == "my_config_value")
                .unwrap()
                .kind,
            "Symbol"
        );
    }

    #[test]
    fn test_extract_decorator_pattern() {
        let entities = extract_entities("@Component class MyUI {}");
        let labels: Vec<&str> = entities.iter().map(|e| e.label.as_str()).collect();
        assert!(labels.contains(&"Component"));
        assert_eq!(
            entities
                .iter()
                .find(|e| e.label == "Component")
                .unwrap()
                .kind,
            "Decorator"
        );
    }

    #[test]
    fn test_multi_language_keywords() {
        let entities =
            extract_entities("interface IService {}; package com.legend; export const X = 1;");
        let labels: Vec<&str> = entities.iter().map(|e| e.label.as_str()).collect();
        assert!(labels.contains(&"IService"));
        assert!(labels.contains(&"com.legend"));
        assert!(labels.contains(&"X"));
    }

    #[test]
    fn test_identifier_normalization_splits_and_singularizes() {
        let ids = extract_identifiers("DataProcessors parse_http_requests and UserIDs");
        assert!(
            ids.contains(&"DataProcessors".to_string()),
            "got: {:?}",
            ids
        );
        assert!(
            ids.contains(&"dataProcessor".to_string())
                || ids.contains(&"DataProcessor".to_string())
                || ids.contains(&"dataprocessor".to_string()),
            "got: {:?}",
            ids
        );
        assert!(ids.contains(&"parse".to_string()), "got: {:?}", ids);
        assert!(ids.contains(&"http".to_string()), "got: {:?}", ids);
        assert!(ids.contains(&"requests".to_string()), "got: {:?}", ids);
        assert!(ids.contains(&"request".to_string()), "got: {:?}", ids);
        assert!(
            ids.contains(&"user".to_string()) || ids.contains(&"User".to_string()),
            "got: {:?}",
            ids
        );
    }
}
