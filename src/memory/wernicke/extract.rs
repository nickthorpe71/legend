use super::cache::KeywordCache;
/// Entity extraction from text (code-aware + plain identifiers).
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Date extraction — temporal perception for the memory system
// ---------------------------------------------------------------------------

/// Month name → number mapping (full and abbreviated).
const MONTHS: &[(&str, &str)] = &[
    ("january", "01"),
    ("february", "02"),
    ("march", "03"),
    ("april", "04"),
    ("may", "05"),
    ("june", "06"),
    ("july", "07"),
    ("august", "08"),
    ("september", "09"),
    ("october", "10"),
    ("november", "11"),
    ("december", "12"),
    ("jan", "01"),
    ("feb", "02"),
    ("mar", "03"),
    ("apr", "04"),
    ("jun", "06"),
    ("jul", "07"),
    ("aug", "08"),
    ("sep", "09"),
    ("oct", "10"),
    ("nov", "11"),
    ("dec", "12"),
];

/// Extract date references from text. Returns deduplicated date strings
/// preserving the original surface form (e.g. "2023/01/15", "March 15th").
pub fn extract_dates(text: &str) -> Vec<String> {
    let mut dates: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Pass 1: ISO-like dates — 2023/01/15, 2023-01-15 (with optional day-of-week suffix)
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;
    while i < len {
        // Look for 4-digit year
        if i + 9 < len && chars[i].is_ascii_digit() {
            let year_str: String = chars[i..i + 4].iter().collect();
            if let Ok(year) = year_str.parse::<u32>() {
                if (1900..=2100).contains(&year) {
                    let sep = chars[i + 4];
                    if (sep == '/' || sep == '-') && i + 9 < len {
                        let month_str: String = chars[i + 5..i + 7].iter().collect();
                        let sep2 = chars[i + 7];
                        let day_str: String = chars[i + 8..i + 10].iter().collect();
                        if sep2 == sep {
                            if let (Ok(m), Ok(d)) =
                                (month_str.parse::<u32>(), day_str.parse::<u32>())
                            {
                                if (1..=12).contains(&m) && (1..=31).contains(&d) {
                                    // Capture the base date
                                    let mut end = i + 10;
                                    // Optionally skip " (Mon)" suffix
                                    if end + 6 <= len && chars[end] == ' ' && chars[end + 1] == '('
                                    {
                                        if let Some(close) =
                                            chars[end + 2..].iter().position(|&c| c == ')')
                                        {
                                            let close_idx = end + 2 + close;
                                            if close_idx - (end + 2) <= 4 {
                                                end = close_idx + 1;
                                            }
                                        }
                                    }
                                    let date: String = chars[i..end].iter().collect();
                                    let key = date.to_lowercase();
                                    if seen.insert(key) {
                                        dates.push(date);
                                    }
                                    i = end;
                                    continue;
                                }
                            }
                        }
                    }
                }
            }
        }
        i += 1;
    }

    // Pass 2: Bracket-wrapped timestamps like [2023/01/15 10:39] or [2023/01/15 (Sun) 10:39]
    // These are already captured by Pass 1 (the date part), so skip.

    // Pass 3: Month-based patterns — "January 15th", "March 2023", "mid-February"
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    let orig_words: Vec<&str> = text.split_whitespace().collect();

    for (wi, word) in words.iter().enumerate() {
        let clean = word.trim_matches(|c: char| !c.is_alphanumeric());

        // Check for qualified months: "mid-February", "early March", "late January"
        let qualifiers = ["mid-", "early ", "late "];
        for q in &qualifiers {
            if let Some(rest) = clean.strip_prefix(q.trim()) {
                let rest_clean = rest.trim_matches('-');
                if MONTHS.iter().any(|(name, _)| rest_clean == *name) {
                    let orig: String = if wi < orig_words.len() {
                        orig_words[wi]
                            .trim_matches(|c: char| matches!(c, ',' | '.' | ';'))
                            .to_string()
                    } else {
                        clean.to_string()
                    };
                    let key = orig.to_lowercase();
                    if seen.insert(key) {
                        dates.push(orig);
                    }
                }
            }
        }

        // Match month name
        if let Some((_, _num)) = MONTHS.iter().find(|(name, _)| clean == *name) {
            // Look ahead for day or year
            if wi + 1 < words.len() {
                let next = words[wi + 1].trim_matches(|c: char| !c.is_alphanumeric());
                // Strip ordinal suffix
                let next_num = next
                    .trim_end_matches("st")
                    .trim_end_matches("nd")
                    .trim_end_matches("rd")
                    .trim_end_matches("th");
                if let Ok(n) = next_num.parse::<u32>() {
                    if (1..=31).contains(&n) {
                        // "January 15th" — month + day
                        let orig = format!(
                            "{} {}",
                            orig_words.get(wi).unwrap_or(&clean),
                            orig_words.get(wi + 1).unwrap_or(&next)
                        )
                        .trim_matches(|c: char| matches!(c, ',' | '.' | ';'))
                        .to_string();
                        let key = orig.to_lowercase();
                        if seen.insert(key) {
                            dates.push(orig);
                        }
                    } else if (1900..=2100).contains(&n) {
                        // "March 2023" — month + year
                        let orig = format!(
                            "{} {}",
                            orig_words.get(wi).unwrap_or(&clean),
                            orig_words.get(wi + 1).unwrap_or(&next)
                        )
                        .trim_matches(|c: char| matches!(c, ',' | '.' | ';'))
                        .to_string();
                        let key = orig.to_lowercase();
                        if seen.insert(key) {
                            dates.push(orig);
                        }
                    }
                }
            }
        }
    }

    // Pass 4: Relative dates — "yesterday", "today", "last week", "3 days ago", etc.
    let relative_single = ["yesterday", "today", "tomorrow"];
    for rel in &relative_single {
        if lower.contains(rel) {
            let key = rel.to_string();
            if seen.insert(key.clone()) {
                dates.push(key);
            }
        }
    }

    // "last <unit>" patterns
    let last_units = [
        "last week",
        "last month",
        "last year",
        "last monday",
        "last tuesday",
        "last wednesday",
        "last thursday",
        "last friday",
        "last saturday",
        "last sunday",
    ];
    for pat in &last_units {
        if lower.contains(pat) {
            let key = pat.to_string();
            if seen.insert(key.clone()) {
                dates.push(key);
            }
        }
    }

    // "N <unit> ago" patterns
    let ago_units = [
        "day", "days", "week", "weeks", "month", "months", "year", "years",
    ];
    for (wi, word) in words.iter().enumerate() {
        if *word == "ago" && wi >= 2 {
            let unit = words[wi - 1].trim_matches(|c: char| !c.is_alphanumeric());
            if ago_units.contains(&unit) {
                let quantity = words[wi - 2].trim_matches(|c: char| !c.is_alphanumeric());
                // Accept numeric or word quantities
                let is_quantity = quantity.parse::<u32>().is_ok()
                    || matches!(
                        quantity,
                        "a" | "one"
                            | "two"
                            | "three"
                            | "four"
                            | "five"
                            | "six"
                            | "seven"
                            | "eight"
                            | "nine"
                            | "ten"
                    );
                if is_quantity {
                    let phrase = format!("{} {} ago", quantity, unit);
                    if seen.insert(phrase.clone()) {
                        dates.push(phrase);
                    }
                }
            }
        }
        // Also handle "a month ago", "a week ago"
        if *word == "ago" && wi >= 2 {
            let unit = words[wi - 1].trim_matches(|c: char| !c.is_alphanumeric());
            let article = words[wi - 2].trim_matches(|c: char| !c.is_alphanumeric());
            if article == "a" && ago_units.contains(&unit) {
                // Already handled above via "a" in is_quantity
            }
        }
    }

    dates
}

pub struct ExtractedEntity {
    pub label: String,
    pub kind: String,
    /// Relationship context: "defines", "uses", "implements", "mentions"
    pub context: String,
}

/// Extract entities from text — multi-pass: code keywords, paths, actions, environments, and identifiers.
pub fn extract_entities(text: &str, kw: &KeywordCache) -> Vec<ExtractedEntity> {
    let mut entities = Vec::new();

    // Pass 0: Date extraction (before other entity types)
    for date in extract_dates(text) {
        entities.push(ExtractedEntity {
            label: date,
            kind: "Date".to_string(),
            context: "mentions".to_string(),
        });
    }

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
        for (verb, kind) in &kw.action {
            if sentence_contains_phrase(&lower, verb) {
                entities.push(ExtractedEntity {
                    label: verb.to_string(),
                    kind: kind.to_string(),
                    context: "performs".to_string(),
                });
            }
        }

        // 3. Environment patterns
        for env in &kw.environment {
            if sentence_contains_phrase(&lower, env) {
                entities.push(ExtractedEntity {
                    label: env.to_string(),
                    kind: "Environment".to_string(),
                    context: "mentions".to_string(),
                });
            }
        }

        // 3b. High-signal technology/tool patterns (constrained dictionary)
        for tool in &kw.tool {
            if sentence_contains_phrase(&lower, tool) {
                entities.push(ExtractedEntity {
                    label: tool.to_string(),
                    kind: "Tool".to_string(),
                    context: "mentions".to_string(),
                });
            }
        }

        // 4. Code patterns (Multi-language)
        for (trigger, kind, ctx, _priority) in &kw.code {
            try_extract(trimmed, trigger, kind, ctx, &mut entities);
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

    // Skip leading quotes if present
    if current.starts_with('\'') || current.starts_with('"') {
        current = &current[1..];
    }

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

    fn kw() -> KeywordCache {
        KeywordCache::default_from_static()
    }

    #[test]
    fn test_extract_entities_rust_code() {
        let entities = extract_entities("fn handle_memory() { struct MemoryState {} }", &kw());
        let labels: Vec<&str> = entities.iter().map(|e| e.label.as_str()).collect();
        assert!(labels.contains(&"handle_memory"), "got: {:?}", labels);
        assert!(labels.contains(&"MemoryState"), "got: {:?}", labels);
    }

    #[test]
    fn test_extract_entities_python() {
        let entities = extract_entities("def process_data(): class DataProcessor:", &kw());
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
        let entities = extract_entities("Fixed the bug in storage. Refactored the TUI.", &kw());
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
        // Domain-independent environment terms are in static Layer 1
        let entities =
            extract_entities("This issue only happens in production and staging.", &kw());
        let labels: Vec<&str> = entities.iter().map(|e| e.label.as_str()).collect();
        assert!(labels.contains(&"production"), "got: {:?}", labels);
        assert!(labels.contains(&"staging"), "got: {:?}", labels);
        assert_eq!(
            entities
                .iter()
                .find(|e| e.label == "production")
                .unwrap()
                .kind,
            "Environment"
        );
    }

    #[test]
    fn test_extract_expanded_actions_and_envs() {
        let entities = extract_entities(
            "Investigating deploy failures in production. We rolled back in staging.",
            &kw(),
        );
        let labels: Vec<&str> = entities.iter().map(|e| e.label.as_str()).collect();
        assert!(labels.contains(&"investigating"), "got: {:?}", labels);
        assert!(labels.contains(&"rolled back"), "got: {:?}", labels);
        assert!(labels.contains(&"production"), "got: {:?}", labels);
        assert!(labels.contains(&"staging"), "got: {:?}", labels);
    }

    #[test]
    fn test_extract_tools_from_bootstrap() {
        // Specific tool names (graphql, redis, etc.) are now seeded via Layer 2
        // (workspace bootstrap). With an empty static tool list, they extract as Terms.
        let entities = extract_entities(
            "We moved services to graphql + redis and added prometheus dashboards.",
            &kw(),
        );
        let labels: Vec<&str> = entities.iter().map(|e| e.label.as_str()).collect();
        // Terms still get extracted as entities, just not classified as "Tool" without bootstrap
        assert!(
            labels.contains(&"graphql")
                || labels.contains(&"redis")
                || labels.contains(&"prometheus"),
            "domain terms should still be extracted, got: {:?}",
            labels
        );

        // With a bootstrapped cache, they'd be classified as Tool
        let mut bootstrapped_kw = kw();
        bootstrapped_kw.tool = vec!["graphql".into(), "redis".into(), "prometheus".into()];
        let entities2 = extract_entities(
            "We moved services to graphql + redis and added prometheus dashboards.",
            &bootstrapped_kw,
        );
        assert_eq!(
            entities2
                .iter()
                .find(|e| e.label == "graphql")
                .unwrap()
                .kind,
            "Tool"
        );
    }

    #[test]
    fn test_extract_assignment_pattern() {
        let entities = extract_entities("my_config_value = 42", &kw());
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
        let entities = extract_entities("@Component class MyUI {}", &kw());
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
        let entities = extract_entities(
            "interface IService {}; package com.legend; export const X = 1;",
            &kw(),
        );
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

    // -----------------------------------------------------------------------
    // extract_after_keyword — edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_extract_after_keyword_basic() {
        assert_eq!(
            extract_after_keyword("fn process_data() {", "fn "),
            Some("process_data".to_string())
        );
    }

    #[test]
    fn test_extract_after_keyword_with_modifiers() {
        assert_eq!(
            extract_after_keyword("pub async fn handle_request()", "fn "),
            Some("handle_request".to_string())
        );
    }

    #[test]
    fn test_extract_after_keyword_class() {
        assert_eq!(
            extract_after_keyword("class MyService {}", "class "),
            Some("MyService".to_string())
        );
    }

    #[test]
    fn test_extract_after_keyword_returns_none_for_stopword() {
        // "the" is a stopword
        assert_eq!(extract_after_keyword("fn the()", "fn "), None);
    }

    #[test]
    fn test_extract_after_keyword_not_at_start() {
        assert_eq!(
            extract_after_keyword("  struct Config {}", "struct "),
            Some("Config".to_string())
        );
    }

    // -----------------------------------------------------------------------
    // extract_entities — code patterns
    // -----------------------------------------------------------------------

    #[test]
    fn test_extract_entities_file_path() {
        let entities = extract_entities("Modified src/commands/memory.rs for the fix", &kw());
        let labels: Vec<&str> = entities.iter().map(|e| e.label.as_str()).collect();
        assert!(
            labels.contains(&"src/commands/memory.rs"),
            "got: {:?}",
            labels
        );
        let fp = entities
            .iter()
            .find(|e| e.label == "src/commands/memory.rs")
            .unwrap();
        assert_eq!(fp.kind, "FilePath");
    }

    #[test]
    fn test_extract_entities_multiple_code_keywords() {
        let entities = extract_entities("fn main() { struct Config {} impl Config {} }", &kw());
        let labels: Vec<&str> = entities.iter().map(|e| e.label.as_str()).collect();
        assert!(labels.contains(&"main"), "got: {:?}", labels);
        assert!(labels.contains(&"Config"), "got: {:?}", labels);
    }

    #[test]
    fn test_extract_entities_deduplicates() {
        let entities = extract_entities("fn Config { struct Config; class Config }", &kw());
        let config_count = entities.iter().filter(|e| e.label == "Config").count();
        assert_eq!(
            config_count,
            1,
            "Should deduplicate: {:?}",
            entities.iter().map(|e| &e.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_extract_entities_empty_input() {
        let entities = extract_entities("", &kw());
        assert!(entities.is_empty());
    }

    #[test]
    fn test_extract_entities_trait_and_impl() {
        let entities = extract_entities(
            "trait Serializable {} impl Serializable for Config {}",
            &kw(),
        );
        let labels: Vec<&str> = entities.iter().map(|e| e.label.as_str()).collect();
        assert!(labels.contains(&"Serializable"), "got: {:?}", labels);
        assert!(labels.contains(&"Config"), "got: {:?}", labels);
    }

    #[test]
    fn test_extract_entities_import_use() {
        let entities = extract_entities("use std::collections::HashMap;", &kw());
        let labels: Vec<&str> = entities.iter().map(|e| e.label.as_str()).collect();
        // The `use` keyword extraction parses the next token; `::` splits into parts
        assert!(
            labels.contains(&"HashMap") || labels.contains(&"std"),
            "Should extract import symbols, got: {:?}",
            labels
        );
    }

    #[test]
    fn test_extract_entities_mixed_content() {
        let entities = extract_entities(
            "Fixed bug in src/parser.rs. Deployed to production using docker. fn handle_request() handles the main API flow.",
            &kw(),
        );
        let labels: Vec<&str> = entities.iter().map(|e| e.label.as_str()).collect();
        assert!(labels.contains(&"src/parser.rs"), "file path: {:?}", labels);
        assert!(labels.contains(&"fixed"), "action: {:?}", labels);
        assert!(labels.contains(&"production"), "env: {:?}", labels);
        assert!(labels.contains(&"docker"), "env: {:?}", labels);
        assert!(labels.contains(&"handle_request"), "fn: {:?}", labels);
    }

    #[test]
    fn test_polyglot_extraction() {
        // Go
        let go = extract_entities("func processTask() { package main }", &kw());
        let go_labels: Vec<&str> = go.iter().map(|e| e.label.as_str()).collect();
        assert!(go_labels.contains(&"processTask"));
        assert!(go_labels.contains(&"main"));

        // TypeScript
        let ts = extract_entities("interface UserData { export const version = 1 }", &kw());
        let ts_labels: Vec<&str> = ts.iter().map(|e| e.label.as_str()).collect();
        assert!(ts_labels.contains(&"UserData"));
        assert!(ts_labels.contains(&"version"));

        // PHP/Ruby
        let web = extract_entities("require 'db_config.php'; module AuthModule {}", &kw());
        let web_labels: Vec<&str> = web.iter().map(|e| e.label.as_str()).collect();
        assert!(web_labels.contains(&"db_config.php"));
        assert!(web_labels.contains(&"AuthModule"));
    }

    #[test]
    fn test_identifier_splitting_edge_cases() {
        // Test that we correctly handle mixed identifiers
        let ids = extract_identifiers("v20_parser_API_Handler");
        assert!(ids.contains(&"v20".to_string()));
        assert!(ids.contains(&"parser".to_string()));
        assert!(ids.contains(&"api".to_string()));
        assert!(ids.contains(&"handler".to_string()));
    }

    // -----------------------------------------------------------------------
    // Date extraction tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_extract_dates_iso_slash() {
        let dates = extract_dates("Visited museum on 2023/01/15 and returned 2023/02/20");
        assert!(
            dates.contains(&"2023/01/15".to_string()),
            "got: {:?}",
            dates
        );
        assert!(
            dates.contains(&"2023/02/20".to_string()),
            "got: {:?}",
            dates
        );
    }

    #[test]
    fn test_extract_dates_iso_dash() {
        let dates = extract_dates("Event on 2024-03-10 was great");
        assert!(
            dates.contains(&"2024-03-10".to_string()),
            "got: {:?}",
            dates
        );
    }

    #[test]
    fn test_extract_dates_iso_with_day_suffix() {
        let dates = extract_dates("[2023/01/15 (Sun) 10:39] Visited the Science Museum");
        assert!(
            dates.iter().any(|d| d.starts_with("2023/01/15")),
            "got: {:?}",
            dates
        );
    }

    #[test]
    fn test_extract_dates_month_day() {
        let dates = extract_dates("We met on January 15th and again March 3rd");
        assert!(
            dates
                .iter()
                .any(|d| d.contains("January") && d.contains("15")),
            "got: {:?}",
            dates
        );
        assert!(
            dates.iter().any(|d| d.contains("March") && d.contains("3")),
            "got: {:?}",
            dates
        );
    }

    #[test]
    fn test_extract_dates_month_year() {
        let dates = extract_dates("Started in March 2023 and ended in January 2024");
        assert!(
            dates
                .iter()
                .any(|d| d.contains("March") && d.contains("2023")),
            "got: {:?}",
            dates
        );
        assert!(
            dates
                .iter()
                .any(|d| d.contains("January") && d.contains("2024")),
            "got: {:?}",
            dates
        );
    }

    #[test]
    fn test_extract_dates_relative() {
        let dates = extract_dates("I did it yesterday and will finish today");
        assert!(dates.contains(&"yesterday".to_string()), "got: {:?}", dates);
        assert!(dates.contains(&"today".to_string()), "got: {:?}", dates);
    }

    #[test]
    fn test_extract_dates_ago() {
        let dates = extract_dates("That happened 3 days ago and two months ago");
        assert!(
            dates.iter().any(|d| d.contains("3 days ago")),
            "got: {:?}",
            dates
        );
        assert!(
            dates.iter().any(|d| d.contains("two months ago")),
            "got: {:?}",
            dates
        );
    }

    #[test]
    fn test_extract_dates_last_patterns() {
        let dates = extract_dates("We discussed it last week and again last Saturday");
        assert!(dates.contains(&"last week".to_string()), "got: {:?}", dates);
        assert!(
            dates.contains(&"last saturday".to_string()),
            "got: {:?}",
            dates
        );
    }

    #[test]
    fn test_extract_dates_deduplication() {
        let dates = extract_dates("On 2023/01/15 we met. Again on 2023/01/15 we talked.");
        let count = dates.iter().filter(|d| d.contains("2023/01/15")).count();
        assert_eq!(count, 1, "should deduplicate: {:?}", dates);
    }

    #[test]
    fn test_extract_dates_no_false_positives() {
        let dates = extract_dates("The function returns 42 and processes 1024 items");
        assert!(
            dates.is_empty(),
            "should not extract non-dates: {:?}",
            dates
        );
    }

    #[test]
    fn test_extract_dates_entities_include_date_kind() {
        let entities = extract_entities("Visited museum on 2023/01/15", &kw());
        let date_entities: Vec<&ExtractedEntity> =
            entities.iter().filter(|e| e.kind == "Date").collect();
        assert!(
            !date_entities.is_empty(),
            "should have Date entities: {:?}",
            entities
                .iter()
                .map(|e| (&e.label, &e.kind))
                .collect::<Vec<_>>()
        );
    }
}
