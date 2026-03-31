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

/// Negative emotional valence keywords — amygdala threat detection.
/// Each tuple: (keyword, valence_weight). Weights are additive; final valence is clamped to [-1, 1].
pub const NEGATIVE_VALENCE_KEYWORDS: &[(&str, f32)] = &[
    // Crashes & panics (high threat)
    ("crash", -0.5),
    ("crashed", -0.5),
    ("crashing", -0.5),
    ("panic", -0.6),
    ("panicked", -0.6),
    ("segfault", -0.7),
    ("segmentation fault", -0.7),
    ("fatal", -0.7),
    ("abort", -0.5),
    ("aborted", -0.5),
    ("oom", -0.6),
    ("out of memory", -0.6),
    ("stack overflow", -0.6),
    ("deadlock", -0.6),
    // Data integrity (very high threat)
    ("data loss", -0.8),
    ("data corruption", -0.8),
    ("corrupt", -0.5),
    ("corrupted", -0.5),
    ("truncated", -0.3),
    ("lost data", -0.8),
    // Bugs & errors
    ("bug:", -0.5),
    ("bug ", -0.4),
    ("error", -0.3),
    ("failure", -0.4),
    ("failed", -0.4),
    ("failing", -0.3),
    ("broken", -0.5),
    ("broke", -0.4),
    ("regression", -0.5),
    ("regressed", -0.5),
    ("breaking change", -0.5),
    ("breaking", -0.4),
    ("undefined behavior", -0.6),
    ("race condition", -0.5),
    ("memory leak", -0.5),
    ("null pointer", -0.5),
    ("nil pointer", -0.5),
    ("index out of bounds", -0.5),
    ("off-by-one", -0.3),
    ("infinite loop", -0.5),
    ("timeout", -0.3),
    ("timed out", -0.3),
    // Security (high threat)
    ("vulnerability", -0.6),
    ("exploit", -0.6),
    ("injection", -0.5),
    ("xss", -0.5),
    ("csrf", -0.5),
    ("security", -0.4),
    ("insecure", -0.5),
    ("unauthorized", -0.4),
    ("authentication failure", -0.5),
    ("privilege escalation", -0.6),
    // Incidents & outages
    ("incident", -0.5),
    ("outage", -0.6),
    ("downtime", -0.5),
    ("degraded", -0.3),
    ("unresponsive", -0.4),
    ("hung", -0.4),
    ("stale", -0.2),
    ("flaky", -0.3),
    // Negative outcomes
    ("reverted", -0.3),
    ("rollback", -0.3),
    ("rolled back", -0.3),
    ("wontfix", -0.2),
    ("deprecated", -0.2),
    ("removed", -0.2),
    ("dropped", -0.3),
];

/// Positive emotional valence keywords — amygdala reward signals.
/// Each tuple: (keyword, valence_weight).
pub const POSITIVE_VALENCE_KEYWORDS: &[(&str, f32)] = &[
    // Completion & shipping (high reward)
    ("shipped", 0.6),
    ("released", 0.5),
    ("deployed", 0.5),
    ("launched", 0.5),
    ("published", 0.4),
    ("completed", 0.5),
    ("finished", 0.4),
    ("done", 0.3),
    ("milestone", 0.5),
    ("delivered", 0.5),
    // Fixes & resolutions
    ("fixed", 0.4),
    ("resolved", 0.4),
    ("patched", 0.3),
    ("remediated", 0.3),
    ("workaround", 0.2),
    // Success & improvement
    ("success", 0.4),
    ("successful", 0.4),
    ("passing", 0.3),
    ("passed", 0.3),
    ("all tests pass", 0.5),
    ("green", 0.2),
    ("improvement", 0.3),
    ("improved", 0.3),
    ("optimized", 0.3),
    ("faster", 0.3),
    ("cleaner", 0.2),
    ("simplified", 0.3),
    ("streamlined", 0.3),
    // Validation & confidence
    ("verified", 0.3),
    ("validated", 0.3),
    ("confirmed", 0.3),
    ("working", 0.2),
    ("stable", 0.3),
    ("robust", 0.3),
    // Progress
    ("progress", 0.2),
    ("merged", 0.3),
    ("approved", 0.3),
    ("accepted", 0.3),
    ("implemented", 0.3),
    ("built", 0.3),
];

/// Urgency keywords that amplify emotional magnitude toward extremes.
pub const URGENCY_KEYWORDS: &[&str] = &[
    "blocker",
    "critical",
    "p0",
    "p1",
    "urgent",
    "emergency",
    "severe",
    "showstopper",
    "hotfix",
    "asap",
    "immediately",
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
