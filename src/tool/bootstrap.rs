/// Workspace keyword bootstrapping — environmental imprinting layer.
///
/// Scans high-signal workspace files (manifests, docs, entry points) to extract
/// domain-specific vocabulary and seed it as `kw:<category>:<term>` graph nodes.
/// This is the second layer of the 3-layer keyword system:
///
/// - **Layer 1** (Innate): Static domain-independent keywords (keywords.rs)
/// - **Layer 2** (Environmental): Workspace-derived keywords (this module)
/// - **Layer 3** (Statistical): Incrementally discovered keywords (9C)
use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::memory::MemoryState;

/// Result of a bootstrap scan — how many keywords were seeded.
pub struct BootstrapResult {
    pub tools: usize,
    pub architecture: usize,
    pub domain: usize,
    pub code: usize,
    pub environment: usize,
    pub total: usize,
}

/// Bootstrap domain-specific keywords from workspace files.
///
/// Reads manifests for dependency names (→ `tool`), documentation for
/// headings and recurring terms (→ `architecture`, `domain`), and detects
/// config/environment patterns (→ `environment`).
///
/// Called during `init` after tier-1 static keyword seeding.
pub fn bootstrap_keywords_from_workspace(
    root: &Path,
    high_signal_files: &[(String, String)], // (path, kind)
    languages: &[(String, usize)],          // (extension, count)
    tech_stack: &[String],
    memory: &mut MemoryState,
) -> BootstrapResult {
    let mut result = BootstrapResult {
        tools: 0,
        architecture: 0,
        domain: 0,
        code: 0,
        environment: 0,
        total: 0,
    };

    let mut seeded: HashSet<String> = HashSet::new();

    // 1. Dependencies → tool keywords
    result.tools += seed_dependencies_as_tools(root, &mut seeded, memory);

    // 2. Tech stack items → tool keywords
    for tech in tech_stack {
        let term = tech.to_lowercase();
        if term.len() >= 2 && seeded.insert(format!("tool:{}", term)) {
            if crate::memory::add_keyword_node(&mut memory.brain,"tool", &term, Vec::new()) {
                result.tools += 1;
            }
        }
    }

    // 3. Scan documentation files → architecture headings + domain terms
    for (path, kind) in high_signal_files {
        let full_path = root.join(path);
        if let Ok(content) = fs::read_to_string(&full_path) {
            match kind.as_str() {
                "Documentation" => {
                    result.architecture +=
                        extract_doc_headings(&content, &mut seeded, memory);
                    result.domain +=
                        extract_recurring_terms(&content, &mut seeded, memory);
                }
                "Manifest" => {
                    // Dependencies already handled above; extract config keys
                    result.environment +=
                        extract_config_keys(&content, path, &mut seeded, memory);
                }
                "EntryPoint" => {
                    // Extract module/struct/type names from entry points
                    result.domain +=
                        extract_code_identifiers(&content, &mut seeded, memory);
                }
                _ => {}
            }
        }
    }

    // 4. Language-specific code keywords based on detected languages
    for (lang, count) in languages {
        if *count > 0 {
            result.code += seed_language_code_keywords(lang, &mut seeded, memory);
        }
    }

    result.total = result.tools + result.architecture + result.domain + result.code + result.environment;
    result
}

/// Parse manifest files and seed dependency names as `kw:tool:<name>`.
fn seed_dependencies_as_tools(
    root: &Path,
    seeded: &mut HashSet<String>,
    memory: &mut MemoryState,
) -> usize {
    let mut count = 0;

    // Rust — Cargo.toml
    if let Ok(content) = fs::read_to_string(root.join("Cargo.toml")) {
        let mut in_deps = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') {
                in_deps = trimmed == "[dependencies]"
                    || trimmed == "[dev-dependencies]"
                    || trimmed == "[build-dependencies]";
                continue;
            }
            if in_deps {
                if let Some(pos) = trimmed.find('=') {
                    let name = trimmed[..pos].trim();
                    if !name.is_empty()
                        && !name.starts_with('#')
                        && name.len() >= 2
                        && seeded.insert(format!("tool:{}", name))
                    {
                        if crate::memory::add_keyword_node(&mut memory.brain,"tool", name, Vec::new()) {
                            count += 1;
                        }
                    }
                }
            }
        }
    }

    // Node.js — package.json
    if let Ok(content) = fs::read_to_string(root.join("package.json")) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            for section in &["dependencies", "devDependencies"] {
                if let Some(deps) = json.get(section).and_then(|v| v.as_object()) {
                    for name in deps.keys() {
                        if name.len() >= 2 && seeded.insert(format!("tool:{}", name)) {
                            if crate::memory::add_keyword_node(&mut memory.brain,"tool", name, Vec::new()) {
                                count += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    // Python — requirements.txt
    if let Ok(content) = fs::read_to_string(root.join("requirements.txt")) {
        for line in content.lines() {
            let name: String = line
                .trim()
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            if name.len() >= 2 && seeded.insert(format!("tool:{}", name)) {
                if crate::memory::add_keyword_node(&mut memory.brain,"tool", &name, Vec::new()) {
                    count += 1;
                }
            }
        }
    }

    // Python — pyproject.toml (basic extraction)
    if let Ok(content) = fs::read_to_string(root.join("pyproject.toml")) {
        let mut in_deps = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') {
                in_deps = trimmed.contains("dependencies");
                continue;
            }
            if in_deps {
                // Lines like: "requests>=2.28" or "\"flask\","
                let cleaned = trimmed.trim_matches(|c: char| c == '"' || c == '\'' || c == ',');
                let name: String = cleaned
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
                    .collect();
                if name.len() >= 2 && seeded.insert(format!("tool:{}", name)) {
                    if crate::memory::add_keyword_node(&mut memory.brain,"tool", &name, Vec::new()) {
                        count += 1;
                    }
                }
            }
        }
    }

    // Go — go.mod
    if let Ok(content) = fs::read_to_string(root.join("go.mod")) {
        for line in content.lines() {
            let trimmed = line.trim();
            // Lines like: "github.com/gin-gonic/gin v1.9.1"
            if trimmed.starts_with("require") || trimmed.is_empty() || trimmed == ")" {
                continue;
            }
            if let Some(path) = trimmed.split_whitespace().next() {
                // Use last segment of module path as the tool name
                if let Some(name) = path.rsplit('/').next() {
                    if name.len() >= 2 && seeded.insert(format!("tool:{}", name)) {
                        if crate::memory::add_keyword_node(&mut memory.brain,"tool", name, Vec::new()) {
                            count += 1;
                        }
                    }
                }
            }
        }
    }

    // Ruby — Gemfile
    if let Ok(content) = fs::read_to_string(root.join("Gemfile")) {
        for line in content.lines() {
            let trimmed = line.trim();
            // Lines like: gem 'rails', '~> 7.0'
            if trimmed.starts_with("gem ") {
                let rest = &trimmed[4..];
                let name: String = rest
                    .trim_start_matches(|c: char| c == '\'' || c == '"')
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
                    .collect();
                if name.len() >= 2 && seeded.insert(format!("tool:{}", name)) {
                    if crate::memory::add_keyword_node(&mut memory.brain,"tool", &name, Vec::new()) {
                        count += 1;
                    }
                }
            }
        }
    }

    count
}

/// Extract markdown headings from documentation as architecture keywords.
fn extract_doc_headings(
    content: &str,
    seeded: &mut HashSet<String>,
    memory: &mut MemoryState,
) -> usize {
    let mut count = 0;

    for line in content.lines() {
        let trimmed = line.trim();
        // Match markdown headings: # Heading, ## Sub-heading, etc.
        if let Some(heading) = trimmed.strip_prefix('#') {
            let heading = heading.trim_start_matches('#').trim();
            // Skip very short or very long headings
            if heading.len() < 3 || heading.len() > 60 {
                continue;
            }
            // Skip generic headings
            if is_generic_heading(heading) {
                continue;
            }
            let lower = heading.to_lowercase();
            if seeded.insert(format!("architecture:{}", lower)) {
                if crate::memory::add_keyword_node(&mut memory.brain,"architecture", &lower, Vec::new()) {
                    count += 1;
                }
            }
        }
    }

    count
}

/// Headings that are too generic to be useful as architecture keywords.
fn is_generic_heading(heading: &str) -> bool {
    let lower = heading.to_lowercase();
    matches!(
        lower.as_str(),
        "introduction"
            | "overview"
            | "getting started"
            | "installation"
            | "usage"
            | "license"
            | "contributing"
            | "changelog"
            | "readme"
            | "table of contents"
            | "toc"
            | "summary"
            | "references"
            | "acknowledgments"
            | "authors"
            | "credits"
            | "faq"
            | "prerequisites"
            | "requirements"
            | "setup"
            | "quick start"
            | "examples"
            | "example"
            | "notes"
            | "note"
            | "warning"
            | "todo"
            | "contents"
    )
}

/// Extract recurring capitalized terms / proper nouns from documentation as domain keywords.
fn extract_recurring_terms(
    content: &str,
    seeded: &mut HashSet<String>,
    memory: &mut MemoryState,
) -> usize {
    let mut term_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for line in content.lines() {
        // Skip headings and code blocks
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.starts_with("```") {
            continue;
        }

        for word in trimmed.split_whitespace() {
            // Clean punctuation
            let clean: String = word
                .trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
                .to_string();

            if clean.len() < 3 || clean.len() > 40 {
                continue;
            }

            // Look for PascalCase, UPPER_CASE, or capitalized terms (not at sentence start)
            let is_pascal = clean.chars().next().map_or(false, |c| c.is_uppercase())
                && clean.chars().any(|c| c.is_lowercase())
                && clean.chars().filter(|c| c.is_uppercase()).count() >= 2;
            let is_upper = clean.len() >= 3
                && clean.chars().all(|c| c.is_uppercase() || c == '_' || c == '-');

            if is_pascal || is_upper {
                *term_counts.entry(clean).or_insert(0) += 1;
            }
        }
    }

    let mut count = 0;
    // Only seed terms that appear 2+ times (reducing noise)
    for (term, freq) in &term_counts {
        if *freq >= 2 {
            let lower = term.to_lowercase();
            if seeded.insert(format!("domain:{}", lower)) {
                if crate::memory::add_keyword_node(&mut memory.brain,"domain", &lower, Vec::new()) {
                    count += 1;
                }
            }
        }
    }

    count
}

/// Extract config/environment keys from manifest files.
fn extract_config_keys(
    content: &str,
    path: &str,
    seeded: &mut HashSet<String>,
    memory: &mut MemoryState,
) -> usize {
    let mut count = 0;
    let lower_path = path.to_lowercase();

    // TOML-style config files: look for [section] headers
    if lower_path.ends_with(".toml") {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                let section = &trimmed[1..trimmed.len() - 1];
                // Skip common sections
                if !matches!(
                    section,
                    "package"
                        | "dependencies"
                        | "dev-dependencies"
                        | "build-dependencies"
                        | "workspace"
                        | "lib"
                        | "bin"
                        | "features"
                        | "profile"
                        | "profile.release"
                        | "profile.dev"
                        | "tool"
                        | "build-system"
                        | "project"
                ) && section.len() >= 3
                {
                    let lower = section.to_lowercase();
                    if seeded.insert(format!("environment:{}", lower)) {
                        if crate::memory::add_keyword_node(&mut memory.brain,"environment", &lower, Vec::new()) {
                            count += 1;
                        }
                    }
                }
            }
        }
    }

    count
}

/// Extract significant code identifiers from entry point files as domain keywords.
fn extract_code_identifiers(
    content: &str,
    seeded: &mut HashSet<String>,
    memory: &mut MemoryState,
) -> usize {
    let mut count = 0;
    let mut ident_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    // Simple pattern: look for PascalCase identifiers (likely type/struct/class names)
    for line in content.lines() {
        let trimmed = line.trim();
        // Skip comments
        if trimmed.starts_with("//") || trimmed.starts_with('#') || trimmed.starts_with("/*") {
            continue;
        }
        for word in trimmed.split(|c: char| !c.is_alphanumeric() && c != '_') {
            if word.len() < 3 || word.len() > 40 {
                continue;
            }
            let is_pascal = word.chars().next().map_or(false, |c| c.is_uppercase())
                && word.chars().any(|c| c.is_lowercase())
                && word.chars().filter(|c| c.is_uppercase()).count() >= 2;

            if is_pascal {
                *ident_counts.entry(word.to_string()).or_insert(0) += 1;
            }
        }
    }

    // Seed identifiers that appear 2+ times
    for (ident, freq) in &ident_counts {
        if *freq >= 2 {
            let lower = ident.to_lowercase();
            if seeded.insert(format!("domain:{}", lower)) {
                if crate::memory::add_keyword_node(&mut memory.brain,"domain", &lower, Vec::new()) {
                    count += 1;
                }
            }
        }
    }

    count
}

/// Language → code keywords mapping for workspace bootstrap seeding.
fn seed_language_code_keywords(
    lang: &str,
    seeded: &mut HashSet<String>,
    memory: &mut MemoryState,
) -> usize {
    let keywords: Vec<(&str, &str, &str, u8)> = match lang.to_lowercase().as_str() {
        "rs" | "rust" => vec![
            ("fn ", "Function", "defines", 7),
            ("struct ", "Struct", "defines", 7),
            ("impl ", "Impl", "implements", 4),
            ("trait ", "Trait", "defines", 7),
            ("enum ", "Enum", "defines", 7),
            ("mod ", "Module", "defines", 7),
        ],
        "py" | "python" => vec![
            ("def ", "Function", "defines", 7),
            ("class ", "Class", "defines", 7),
        ],
        "js" | "ts" | "jsx" | "tsx" | "javascript" | "typescript" => vec![
            ("function ", "Function", "defines", 7),
            ("interface ", "Interface", "defines", 7),
            ("export ", "Export", "defines", 4),
            ("import ", "Import", "uses", 4),
            ("const ", "Symbol", "defines", 5),
            ("let ", "Symbol", "defines", 5),
        ],
        "go" => vec![
            ("func ", "Function", "defines", 7),
            ("package ", "Package", "defines", 4),
        ],
        "rb" | "ruby" => vec![
            ("module ", "Module", "defines", 7),
            ("require ", "Import", "uses", 4),
            ("class ", "Class", "defines", 7),
            ("def ", "Function", "defines", 7),
        ],
        "php" => vec![
            ("class ", "Class", "defines", 7),
            ("function ", "Function", "defines", 7),
            ("namespace ", "Module", "defines", 7),
            ("use ", "Import", "uses", 4),
        ],
        "java" | "kt" | "kotlin" => vec![
            ("class ", "Class", "defines", 7),
            ("interface ", "Interface", "defines", 7),
            ("package ", "Package", "defines", 4),
            ("import ", "Import", "uses", 4),
        ],
        "cs" | "csharp" => vec![
            ("class ", "Class", "defines", 7),
            ("interface ", "Interface", "defines", 7),
            ("namespace ", "Module", "defines", 7),
            ("using ", "Import", "uses", 4),
        ],
        "c" | "cpp" | "h" | "hpp" => vec![
            ("struct ", "Struct", "defines", 7),
            ("#include ", "Import", "uses", 4),
            ("typedef ", "Type", "defines", 5),
        ],
        "swift" => vec![
            ("func ", "Function", "defines", 7),
            ("struct ", "Struct", "defines", 7),
            ("class ", "Class", "defines", 7),
            ("protocol ", "Interface", "defines", 7),
            ("import ", "Import", "uses", 4),
        ],
        "zig" => vec![
            ("fn ", "Function", "defines", 7),
            ("const ", "Symbol", "defines", 5),
            ("pub fn ", "Function", "defines", 7),
        ],
        _ => Vec::new(),
    };

    let mut count = 0;
    for (trigger, kind, ctx, pri) in keywords {
        if seeded.insert(format!("code:{}", trigger)) {
            let metadata = vec![
                format!("entity_kind:{}", kind),
                format!("entity_context:{}", ctx),
                format!("entity_priority:{}", pri),
            ];
            if crate::memory::add_keyword_node(&mut memory.brain,"code", trigger, metadata) {
                count += 1;
            }
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fresh_memory() -> MemoryState {
        MemoryState::default()
    }

    #[test]
    fn test_bootstrap_empty_workspace() {
        let dir = TempDir::new().unwrap();
        let mut memory = fresh_memory();
        let result = bootstrap_keywords_from_workspace(
            dir.path(),
            &[],
            &[],
            &[],
            &mut memory,
        );
        assert_eq!(result.total, 0);
    }

    #[test]
    fn test_bootstrap_extracts_cargo_dependencies_as_tools() {
        let dir = TempDir::new().unwrap();
        let cargo_toml = dir.path().join("Cargo.toml");
        fs::write(
            &cargo_toml,
            r#"[package]
name = "test-project"
version = "0.1.0"

[dependencies]
serde = "1.0"
tokio = { version = "1", features = ["full"] }

[dev-dependencies]
tempfile = "3"
"#,
        )
        .unwrap();

        let mut memory = fresh_memory();
        let result = bootstrap_keywords_from_workspace(
            dir.path(),
            &[("Cargo.toml".to_string(), "Manifest".to_string())],
            &[("rs".to_string(), 5)],
            &[],
            &mut memory,
        );

        assert!(result.tools >= 3, "should extract serde, tokio, tempfile; got {}", result.tools);

        // Verify they're in the graph as kw:tool:* nodes
        assert!(memory.brain.long_term.index.contains_key("kw:tool:serde"));
        assert!(memory.brain.long_term.index.contains_key("kw:tool:tokio"));
        assert!(memory.brain.long_term.index.contains_key("kw:tool:tempfile"));
    }

    #[test]
    fn test_bootstrap_extracts_package_json_dependencies() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{
  "dependencies": {
    "react": "^18.0",
    "express": "^4.18"
  },
  "devDependencies": {
    "jest": "^29.0"
  }
}"#,
        )
        .unwrap();

        let mut memory = fresh_memory();
        let result = bootstrap_keywords_from_workspace(
            dir.path(),
            &[],
            &[("js".to_string(), 10)],
            &[],
            &mut memory,
        );

        assert!(result.tools >= 3, "should extract react, express, jest; got {}", result.tools);
        assert!(memory.brain.long_term.index.contains_key("kw:tool:react"));
        assert!(memory.brain.long_term.index.contains_key("kw:tool:express"));
    }

    #[test]
    fn test_bootstrap_extracts_doc_headings_as_architecture() {
        let dir = TempDir::new().unwrap();
        let readme = dir.path().join("README.md");
        fs::write(
            &readme,
            r#"# My Project

## Overview

## Memory System Architecture

## Query Processing Pipeline

## Getting Started
"#,
        )
        .unwrap();

        let mut memory = fresh_memory();
        let result = bootstrap_keywords_from_workspace(
            dir.path(),
            &[("README.md".to_string(), "Documentation".to_string())],
            &[],
            &[],
            &mut memory,
        );

        // "Overview" and "Getting Started" are generic and should be filtered
        // "Memory System Architecture" and "Query Processing Pipeline" should be extracted
        assert!(
            result.architecture >= 2,
            "should extract non-generic headings; got {}",
            result.architecture
        );
        assert!(memory
            .brain.long_term
            .index
            .contains_key("kw:architecture:memory system architecture"));
        assert!(memory
            .brain.long_term
            .index
            .contains_key("kw:architecture:query processing pipeline"));
    }

    #[test]
    fn test_bootstrap_generic_headings_filtered() {
        let dir = TempDir::new().unwrap();
        let readme = dir.path().join("README.md");
        fs::write(
            &readme,
            "# Installation\n## Usage\n## License\n## Contributing\n",
        )
        .unwrap();

        let mut memory = fresh_memory();
        let result = bootstrap_keywords_from_workspace(
            dir.path(),
            &[("README.md".to_string(), "Documentation".to_string())],
            &[],
            &[],
            &mut memory,
        );

        assert_eq!(result.architecture, 0, "generic headings should be filtered");
    }

    #[test]
    fn test_bootstrap_extracts_recurring_pascal_case_as_domain() {
        let dir = TempDir::new().unwrap();
        let readme = dir.path().join("README.md");
        fs::write(
            &readme,
            "The MemoryState handles all persistence.\n\
             You can query MemoryState for context.\n\
             GraphMemory stores the knowledge graph.\n\
             GraphMemory enables spreading activation.\n",
        )
        .unwrap();

        let mut memory = fresh_memory();
        let result = bootstrap_keywords_from_workspace(
            dir.path(),
            &[("README.md".to_string(), "Documentation".to_string())],
            &[],
            &[],
            &mut memory,
        );

        // MemoryState and GraphMemory appear 2+ times each
        assert!(result.domain >= 2, "should extract recurring PascalCase terms; got {}", result.domain);
        assert!(memory.brain.long_term.index.contains_key("kw:domain:memorystate"));
        assert!(memory.brain.long_term.index.contains_key("kw:domain:graphmemory"));
    }

    #[test]
    fn test_bootstrap_single_occurrence_terms_not_seeded() {
        let dir = TempDir::new().unwrap();
        let readme = dir.path().join("README.md");
        fs::write(&readme, "The UniqueWidget handles display.\n").unwrap();

        let mut memory = fresh_memory();
        let result = bootstrap_keywords_from_workspace(
            dir.path(),
            &[("README.md".to_string(), "Documentation".to_string())],
            &[],
            &[],
            &mut memory,
        );

        assert_eq!(result.domain, 0, "single-occurrence terms should not be seeded");
    }

    #[test]
    fn test_bootstrap_seeds_language_code_keywords() {
        let dir = TempDir::new().unwrap();
        let mut memory = fresh_memory();
        let result = bootstrap_keywords_from_workspace(
            dir.path(),
            &[],
            &[("rs".to_string(), 10), ("py".to_string(), 3)],
            &[],
            &mut memory,
        );

        // Rust: fn, struct, impl, trait, enum, mod = 6
        // Python: def, class = 2
        assert!(result.code >= 8, "should seed language code keywords; got {}", result.code);
        assert!(memory.brain.long_term.index.contains_key("kw:code:fn "));
        assert!(memory.brain.long_term.index.contains_key("kw:code:struct "));
        assert!(memory.brain.long_term.index.contains_key("kw:code:def "));
    }

    #[test]
    fn test_bootstrap_tech_stack_as_tools() {
        let dir = TempDir::new().unwrap();
        let mut memory = fresh_memory();
        let result = bootstrap_keywords_from_workspace(
            dir.path(),
            &[],
            &[],
            &["React".to_string(), "TypeScript".to_string()],
            &mut memory,
        );

        assert!(result.tools >= 2, "tech stack items should become tools; got {}", result.tools);
        assert!(memory.brain.long_term.index.contains_key("kw:tool:react"));
        assert!(memory.brain.long_term.index.contains_key("kw:tool:typescript"));
    }

    #[test]
    fn test_bootstrap_no_duplicate_seeding() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[dependencies]\nserde = \"1.0\"\n",
        )
        .unwrap();

        let mut memory = fresh_memory();
        // Bootstrap twice
        bootstrap_keywords_from_workspace(dir.path(), &[], &[], &[], &mut memory);
        let result = bootstrap_keywords_from_workspace(dir.path(), &[], &[], &[], &mut memory);

        // Second run should seed 0 new (add_keyword_node returns false for existing)
        assert_eq!(result.tools, 0, "second bootstrap should not duplicate keywords");
    }

    #[test]
    fn test_bootstrap_requirements_txt() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("requirements.txt"),
            "flask>=2.0\nrequests==2.28.1\nnumpy\n",
        )
        .unwrap();

        let mut memory = fresh_memory();
        let result = bootstrap_keywords_from_workspace(
            dir.path(),
            &[],
            &[("py".to_string(), 5)],
            &[],
            &mut memory,
        );

        assert!(result.tools >= 3, "should extract flask, requests, numpy; got {}", result.tools);
        assert!(memory.brain.long_term.index.contains_key("kw:tool:flask"));
        assert!(memory.brain.long_term.index.contains_key("kw:tool:requests"));
        assert!(memory.brain.long_term.index.contains_key("kw:tool:numpy"));
    }

    #[test]
    fn test_bootstrap_entry_point_extracts_code_identifiers() {
        let dir = TempDir::new().unwrap();
        let main_rs = dir.path().join("main.rs");
        fs::write(
            &main_rs,
            r#"use crate::AppConfig;
fn main() {
    let config = AppConfig::new();
    let state = AppConfig::load();
    let handler = RequestHandler::new();
    let handler2 = RequestHandler::process();
}
"#,
        )
        .unwrap();

        let mut memory = fresh_memory();
        let result = bootstrap_keywords_from_workspace(
            dir.path(),
            &[("main.rs".to_string(), "EntryPoint".to_string())],
            &[],
            &[],
            &mut memory,
        );

        // AppConfig appears 3x, RequestHandler 2x
        assert!(result.domain >= 2, "should extract PascalCase identifiers; got {}", result.domain);
        assert!(memory.brain.long_term.index.contains_key("kw:domain:appconfig"));
        assert!(memory.brain.long_term.index.contains_key("kw:domain:requesthandler"));
    }
}
