/// Centralized semantic dictionaries for entity extraction and summarization.
///
/// Layer 1 (Innate): Domain-independent keyword lists that work across any project.
/// These are like hardwired neural circuits — decision detection, threat signals,
/// reward signals, urgency amplifiers. They function regardless of domain.
///
/// Domain-specific keywords (language syntax, tools, environments) are seeded
/// via Layer 2 (workspace bootstrap) and Layer 3 (incremental discovery).

/// Code syntax keywords — language-specific patterns that indicate definitions/usage.
/// NOTE: These are seeded during workspace bootstrap (Layer 2) based on detected
/// languages, NOT loaded as static defaults. Kept here as a reference catalog.
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

/// Decision-making language — domain-independent reasoning markers.
pub const DECISION_KEYWORDS: &[&str] = &[
    // Causal reasoning
    "because",
    "reason",
    "rationale",
    "therefore",
    "consequently",
    "hence",
    "thus",
    "given that",
    "in order to",
    "so that",
    "the reason",
    "due to",
    "as a result",
    // Choice language
    "chose",
    "decided",
    "decision",
    "opted",
    "opted for",
    "picked",
    "selected",
    "went with",
    "settled on",
    "committed to",
    "we decided",
    // Comparison/alternatives
    "instead",
    "rather",
    "over",
    "alternatively",
    "compared to",
    "versus",
    "vs",
    "tradeoff",
    "trade-off",
    "tradeoffs",
    "pros and cons",
    "weighed",
    // Evaluation
    "approach",
    "strategy",
    "evaluated",
    "considered",
    "concluded",
    "determined",
    "assessed",
    "analyzed",
    // Rejection
    "rejected",
    "ruled out",
    "discarded",
    "abandoned",
    "dropped",
    "not worth",
    "too complex",
];

/// Action verbs — domain-independent progress markers.
pub const ACTION_KEYWORDS: &[(&str, &str)] = &[
    // Building
    ("implemented", "Action"),
    ("implementing", "Action"),
    ("added", "Action"),
    ("adding", "Action"),
    ("created", "Action"),
    ("creating", "Action"),
    ("built", "Action"),
    ("building", "Action"),
    ("designed", "Action"),
    ("designing", "Action"),
    ("developed", "Action"),
    ("developing", "Action"),
    ("wrote", "Action"),
    ("writing", "Action"),
    ("set up", "Action"),
    ("configured", "Action"),
    ("integrated", "Action"),
    // Fixing
    ("fixed", "Action"),
    ("fixing", "Action"),
    ("resolved", "Action"),
    ("resolving", "Action"),
    ("patched", "Action"),
    ("addressed", "Action"),
    ("corrected", "Action"),
    // Modifying
    ("refactored", "Action"),
    ("refactoring", "Action"),
    ("updated", "Action"),
    ("updating", "Action"),
    ("changed", "Action"),
    ("modified", "Action"),
    ("replaced", "Action"),
    ("renamed", "Action"),
    ("restructured", "Action"),
    ("reorganized", "Action"),
    ("consolidated", "Action"),
    ("simplified", "Action"),
    // Removing
    ("removed", "Action"),
    ("removing", "Action"),
    ("deleted", "Action"),
    ("cleaned up", "Action"),
    ("pruned", "Action"),
    ("deprecated", "Action"),
    // Testing/validation
    ("tested", "Action"),
    ("testing", "Action"),
    ("debugged", "Action"),
    ("debugging", "Action"),
    ("validated", "Action"),
    ("validating", "Action"),
    ("verified", "Action"),
    ("profiled", "Action"),
    ("benchmarked", "Action"),
    // Optimization
    ("optimized", "Action"),
    ("optimizing", "Action"),
    ("improved", "Action"),
    ("streamlined", "Action"),
    // Movement/deployment
    ("migrated", "Action"),
    ("migrating", "Action"),
    ("deployed", "Action"),
    ("deploying", "Action"),
    ("reverted", "Action"),
    ("reverting", "Action"),
    ("rolled back", "Action"),
    ("rollback", "Action"),
    ("upgraded", "Action"),
    ("downgraded", "Action"),
    // Investigation
    ("investigated", "Action"),
    ("investigating", "Action"),
    ("researched", "Action"),
    ("explored", "Action"),
    ("analyzed", "Action"),
    ("diagnosed", "Action"),
    // Documentation
    ("documented", "Action"),
    ("documenting", "Action"),
    // Completion
    ("completed", "Action"),
    ("finished", "Action"),
    ("shipped", "Action"),
    ("merged", "Action"),
    ("released", "Action"),
    ("published", "Action"),
];

/// Architecture/design language — structural concepts that apply across domains.
pub const ARCHITECTURE_KEYWORDS: &[&str] = &[
    // Core structural concepts
    "architecture",
    "module",
    "component",
    "layer",
    "system",
    "subsystem",
    "service",
    "interface",
    "api",
    "schema",
    "model",
    // Patterns & design
    "pattern",
    "pipeline",
    "middleware",
    "handler",
    "controller",
    "registry",
    "dispatcher",
    "orchestrator",
    "scheduler",
    "facade",
    "adapter",
    "proxy",
    "gateway",
    "bridge",
    "factory",
    "singleton",
    "observer",
    "strategy",
    // Boundaries & communication
    "boundary",
    "protocol",
    "contract",
    "endpoint",
    "route",
    "channel",
    "queue",
    "stream",
    "event",
    "message",
    "signal",
    // Data organization
    "repository",
    "store",
    "cache",
    "index",
    "graph",
    "tree",
    "hierarchy",
    "namespace",
    "scope",
    // Process & flow
    "workflow",
    "lifecycle",
    "state machine",
    "transition",
    "hook",
    "callback",
    "plugin",
    "extension",
];

/// Bug/issue language — domain-independent problem indicators.
pub const BUG_KEYWORDS: &[&str] = &[
    // Direct bug references
    "bug",
    "bug:",
    "defect",
    "glitch",
    "fault",
    // Breakage
    "broke",
    "broken",
    "breaking",
    "crash",
    "panic",
    // Regression
    "regression",
    "regressed",
    "revert",
    "reverted",
    // Failure
    "fix",
    "hotfix",
    "incident",
    "error",
    "failure",
    "failed",
    "failing",
    // Quality issues
    "anomaly",
    "deviation",
    "malfunction",
    "degradation",
    "inconsistency",
    "flaky",
    "unstable",
    "unreliable",
    "intermittent",
    // Symptoms
    "hang",
    "hung",
    "freeze",
    "timeout",
    "leak",
    "overflow",
    "corruption",
];

/// Todo/task language — outstanding work markers.
pub const TODO_KEYWORDS: &[&str] = &[
    // Direct markers
    "todo",
    "fixme",
    "hack",
    "xxx",
    "temporary",
    // Status
    "still need",
    "not yet",
    "remaining",
    "outstanding",
    "pending",
    "incomplete",
    "unfinished",
    // Priority/tracking
    "blocker",
    "blocked",
    "backlog",
    "deferred",
    "scheduled",
    "planned",
    "next step",
    "follow-up",
    "follow up",
    // Needs
    "needs work",
    "needs review",
    "needs testing",
    "needs refactor",
    "must fix",
    "should fix",
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
    "high priority",
    "time-sensitive",
];

/// Preference/convention language — user style and workflow markers.
pub const PREFERENCE_KEYWORDS: &[&str] = &[
    "prefer",
    "preference",
    "user wants",
    "user prefers",
    "style",
    "convention",
    "standard",
    "rule",
    "guideline",
    "policy",
    "always use",
    "never use",
    "default to",
    "should always",
    "should never",
    "must use",
    "avoid",
];

/// Environment keywords — deployment/infrastructure context.
/// NOTE: Specific platform names (docker, kubernetes, aws, etc.) are seeded via
/// Layer 2 (workspace bootstrap). These are domain-independent environment concepts.
pub const ENVIRONMENT_KEYWORDS: &[&str] = &[
    "production",
    "staging",
    "development",
    "local",
    "localhost",
    "remote",
    "ci",
    "cd",
    "pipeline",
    "deployment",
    "infrastructure",
    "configuration",
    "environment",
    "runtime",
    "server",
    "client",
    "browser",
    "cli",
    "vm",
    "container",
];

/// Tool keywords — domain-independent. Specific tool names are seeded via Layer 2.
/// This static list is intentionally empty; all tool keywords come from workspace
/// bootstrap (Layer 2) based on detected dependencies and tech stack.
pub const TOOL_KEYWORDS: &[&str] = &[];

/// Domain keywords — initially empty. Populated entirely from graph nodes via
/// Layer 2 (workspace bootstrap) and Layer 3 (incremental discovery).
pub const DOMAIN_KEYWORDS: &[&str] = &[];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_independent_lists_not_empty() {
        assert!(!DECISION_KEYWORDS.is_empty());
        assert!(!ACTION_KEYWORDS.is_empty());
        assert!(!ARCHITECTURE_KEYWORDS.is_empty());
        assert!(!BUG_KEYWORDS.is_empty());
        assert!(!TODO_KEYWORDS.is_empty());
        assert!(!PREFERENCE_KEYWORDS.is_empty());
        assert!(!ENVIRONMENT_KEYWORDS.is_empty());
        assert!(!URGENCY_KEYWORDS.is_empty());
    }

    #[test]
    fn test_code_keywords_are_reference_catalog() {
        // Code keywords exist as a reference but are seeded via Layer 2
        assert!(!CODE_KEYWORDS.is_empty());
    }

    #[test]
    fn test_tool_keywords_empty_static() {
        // Tools come from workspace bootstrap, not static
        assert!(TOOL_KEYWORDS.is_empty());
    }

    #[test]
    fn test_domain_keywords_empty_static() {
        // Domain terms come from bootstrap + incremental discovery
        assert!(DOMAIN_KEYWORDS.is_empty());
    }

    #[test]
    fn test_code_keyword_format() {
        for (kw, kind, ctx) in CODE_KEYWORDS {
            assert!(!kw.is_empty(), "Keyword cannot be empty");
            assert!(!kind.is_empty(), "Kind cannot be empty");
            assert!(!ctx.is_empty(), "Context cannot be empty");
            // Most keywords should end with a space to avoid false positives
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
        assert!(!has_duplicates(ARCHITECTURE_KEYWORDS), "Duplicate in ARCHITECTURE_KEYWORDS");
        assert!(!has_duplicates(BUG_KEYWORDS), "Duplicate in BUG_KEYWORDS");
        assert!(!has_duplicates(PREFERENCE_KEYWORDS), "Duplicate in PREFERENCE_KEYWORDS");
        assert!(!has_duplicates(URGENCY_KEYWORDS), "Duplicate in URGENCY_KEYWORDS");
    }

    #[test]
    fn test_environment_keywords_are_domain_independent() {
        // No specific vendor/platform names in static list
        for kw in ENVIRONMENT_KEYWORDS {
            let lower = kw.to_lowercase();
            assert!(
                !["docker", "kubernetes", "k8s", "aws", "gcp", "azure",
                  "github", "terraform", "ansible"].contains(&lower.as_str()),
                "Domain-specific platform '{}' should not be in static ENVIRONMENT_KEYWORDS", kw
            );
        }
    }

    #[test]
    fn test_valence_weights_in_range() {
        for (kw, w) in NEGATIVE_VALENCE_KEYWORDS {
            assert!(*w >= -1.0 && *w <= 0.0, "Negative valence '{}' weight {} out of range", kw, w);
        }
        for (kw, w) in POSITIVE_VALENCE_KEYWORDS {
            assert!(*w >= 0.0 && *w <= 1.0, "Positive valence '{}' weight {} out of range", kw, w);
        }
    }
}
