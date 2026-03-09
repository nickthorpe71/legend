/// Centralized semantic dictionaries for entity extraction and summarization.
pub const CODE_KEYWORDS: &[(&str, &str, &str)] = &[
    // Rust / C / C++
    ("fn ", "Function", "defines"),
    ("struct ", "Struct", "defines"),
    ("impl ", "Impl", "implements"),
    ("trait ", "Trait", "defines"),
    ("enum ", "Enum", "defines"),
    ("mod ", "Module", "defines"),
    // Python / Mojo
    ("def ", "Function", "defines"),
    ("class ", "Class", "defines"),
    // JavaScript / TypeScript
    ("function ", "Function", "defines"),
    ("interface ", "Interface", "defines"),
    ("export ", "Export", "defines"),
    ("import ", "Import", "uses"),
    ("const ", "Symbol", "defines"),
    ("let ", "Symbol", "defines"),
    // Go
    ("func ", "Function", "defines"),
    ("package ", "Package", "defines"),
    // Ruby / PHP
    ("module ", "Module", "defines"),
    ("require ", "Import", "uses"),
];

pub const DECISION_KEYWORDS: &[&str] = &[
    "because",
    "chose",
    "decided",
    "decision",
    "instead",
    "rather",
    "reason",
    "rationale",
    "tradeoff",
    "trade-off",
    "over",
    "picked",
    "opted",
    "went with",
    "rejected",
    "approach",
];

pub const ACTION_KEYWORDS: &[(&str, &str)] = &[
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
    // Plain verbs for progress categorization
    ("completed", "Action"),
    ("finished", "Action"),
    ("built", "Action"),
    ("shipped", "Action"),
    ("merged", "Action"),
];

pub const ENVIRONMENT_KEYWORDS: &[&str] = &[
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

pub const TOOL_KEYWORDS: &[&str] = &[
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

pub const ARCHITECTURE_KEYWORDS: &[&str] = &[
    "architecture",
    "module",
    "component",
    "layer",
    "system",
    "interface",
    "api",
    "schema",
    "pipeline",
    "pattern",
    "struct ",
    "trait ",
    "impl ",
];

pub const BUG_KEYWORDS: &[&str] = &[
    "bug",
    "broke",
    "broken",
    "revert",
    "reverted",
    "crash",
    "panic",
    "regression",
    "fix",
    "hotfix",
    "incident",
    "error",
    "failure",
    "failed",
];

pub const TODO_KEYWORDS: &[&str] = &[
    "todo",
    "fixme",
    "hack",
    "still need",
    "not yet",
    "remaining",
    "blocker",
    "blocked",
];

pub const PREFERENCE_KEYWORDS: &[&str] = &[
    "prefer",
    "preference",
    "user wants",
    "user prefers",
    "style",
    "convention",
    "always use",
    "never use",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dictionaries_not_empty() {
        assert!(!CODE_KEYWORDS.is_empty());
        assert!(!DECISION_KEYWORDS.is_empty());
        assert!(!ACTION_KEYWORDS.is_empty());
        assert!(!ENVIRONMENT_KEYWORDS.is_empty());
        assert!(!TOOL_KEYWORDS.is_empty());
        assert!(!ARCHITECTURE_KEYWORDS.is_empty());
        assert!(!BUG_KEYWORDS.is_empty());
        assert!(!TODO_KEYWORDS.is_empty());
        assert!(!PREFERENCE_KEYWORDS.is_empty());
    }

    #[test]
    fn test_code_keyword_format() {
        for (kw, kind, ctx) in CODE_KEYWORDS {
            assert!(!kw.is_empty(), "Keyword cannot be empty");
            assert!(!kind.is_empty(), "Kind cannot be empty");
            assert!(!ctx.is_empty(), "Context cannot be empty");
            // Most keywords should end with a space to avoid false positives like 'functional' matching 'fn'
            if kw.len() <= 3 {
                assert!(kw.ends_with(' '), "Short keyword '{}' must end with a space", kw);
            }
        }
    }

    #[test]
    fn test_action_keyword_format() {
        for (verb, kind) in ACTION_KEYWORDS {
            assert!(!verb.is_empty());
            assert_eq!(*kind, "Action");
        }
    }

    #[test]
    fn test_no_duplicate_keywords_in_lists() {
        use std::collections::HashSet;
        
        fn has_duplicates(list: &[&str]) -> bool {
            let mut seen = HashSet::new();
            for item in list {
                if !seen.insert(item.to_lowercase()) {
                    return true;
                }
            }
            false
        }

        assert!(!has_duplicates(DECISION_KEYWORDS), "Duplicate in DECISION_KEYWORDS");
        assert!(!has_duplicates(ENVIRONMENT_KEYWORDS), "Duplicate in ENVIRONMENT_KEYWORDS");
        assert!(!has_duplicates(TODO_KEYWORDS), "Duplicate in TODO_KEYWORDS");
    }
}
