/// Thalamus — Attentional gating and salience scoring.
///
/// The thalamus is the brain's central relay station, routing sensory input
/// to appropriate cortical regions and — critically — gating what gets
/// through based on attentional priority.  Not all incoming signals are
/// equal: the thalamus amplifies salient stimuli and suppresses noise.
///
/// In Legend, `thalamus.rs` implements this attentional gating role:
///
/// - **Salience scoring (`compute_salience`)** — assigns an importance prior
///   to each incoming chunk based on domain-general cognitive signals
///   (decisions, bugs, preferences, blockers) plus learned domain vocabulary.
///   This determines whether the prefrontal attention gate promotes the input
///   to episodic memory (L2) or lets it fade in working memory (L1).
///
/// The thalamus is deliberately stateless: it scores input and returns
/// values without mutating `BrainState`.  All side effects happen in the
/// calling orchestrator (`tick_impl`) and downstream brain-region modules.
///
/// Representational encoding (embeddings, cosine similarity) lives in
/// `entorhinal.rs`, which owns the full encoding-and-compression pipeline.
use crate::memory::wernicke::KeywordCache;

/// Compute salience score from text content heuristics.
pub fn compute_salience(text: &str, kw: &KeywordCache) -> f32 {
    let mut score: f32 = 0.0;
    let lowered = text.to_lowercase();

    // Decision language — highest importance
    let decision_hits = kw
        .decision
        .iter()
        .filter(|k| lowered.contains(k.as_str()))
        .count();
    if decision_hits >= 2 {
        score += 0.5;
    } else if decision_hits >= 1 {
        score += 0.3;
    }
    // Rationale language amplifies decisions
    if decision_hits > 0
        && (lowered.contains("because")
            || lowered.contains("rationale")
            || lowered.contains("reason"))
    {
        score += 0.15;
    }

    // Bug/incident language — high importance
    if kw.bug.iter().any(|k| lowered.contains(k.as_str())) {
        score += 0.4;
    }

    // TODO/blocker language
    if kw.todo.iter().any(|k| lowered.contains(k.as_str())) {
        score += 0.3;
    }

    // Architecture/structural statements. This is intentionally modest:
    // domain-specific architecture should become salient through learned
    // vocabulary instead of hardcoded software terms.
    if kw.architecture.iter().any(|k| lowered.contains(k.as_str())) {
        score += 0.15;
    }

    // Preference/convention
    if kw.preference.iter().any(|k| lowered.contains(k.as_str())) {
        score += 0.3;
    }

    // Domain-specific vocabulary learned from the workspace. Repeated,
    // meaningful domain terms should outweigh syntax cues from any single
    // domain (including code), but the contribution stays bounded so learned
    // noise cannot saturate salience by itself.
    let domain_hits = kw
        .domain
        .iter()
        .filter(|k| lowered.contains(k.as_str()))
        .count();
    if domain_hits > 0 {
        score += (0.18 + 0.08 * (domain_hits.saturating_sub(1) as f32)).min(0.4);
    }

    // Code syntax is a weak density cue, not an innate priority. Code remains
    // important when paired with generic cognitive signals such as decisions,
    // bugs, blockers, or learned domain vocabulary.
    if text.contains("```") {
        score += 0.03;
    }
    let code_def_hits = kw
        .code
        .iter()
        .filter(|(trigger, _, _, _)| lowered.contains(trigger.as_str()))
        .count();
    if code_def_hits >= 2 {
        score += 0.05;
    } else if code_def_hits == 1 {
        score += 0.03;
    }

    // Substantive text (not too short)
    let word_count = text.split_whitespace().count();
    if word_count > 50 {
        score += 0.2;
    } else if word_count > 25 {
        score += 0.15;
    }

    // Error mentions
    if lowered.contains("error") {
        score += 0.15;
    }

    score.clamp(0.05, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::wernicke::KeywordCache;

    fn kw() -> KeywordCache {
        KeywordCache::default_from_static()
    }

    #[test]
    fn test_compute_salience_code_syntax_is_weak_by_itself() {
        let s = compute_salience("fn main() { struct Foo {} }", &kw());
        assert!(
            s < 0.25,
            "code syntax alone should not cross the attention gate, got {}",
            s
        );
    }

    #[test]
    fn test_compute_salience_learned_domain_beats_code_syntax() {
        let mut kw = kw();
        kw.domain = vec!["project alpha".to_string(), "sqlite".to_string()];

        let domain = compute_salience(
            "Project Alpha stores session history in SQLite for local recall",
            &kw,
        );
        let code = compute_salience("fn main() { struct SessionStore {} }", &kw);

        assert!(
            domain > code,
            "learned domain vocabulary should dominate code syntax: {} vs {}",
            domain,
            code
        );
        assert!(
            domain >= 0.25,
            "two learned domain terms should cross the attention gate, got {}",
            domain
        );
    }

    #[test]
    fn test_compute_salience_code_with_generic_importance_stays_high() {
        let s = compute_salience(
            "Bug: fn load_memory() panics when SQLite returns an empty row",
            &kw(),
        );
        assert!(
            s >= 0.4,
            "code-related bug should stay high through bug salience, got {}",
            s
        );
    }

    #[test]
    fn test_compute_salience_todo() {
        assert!(
            compute_salience("TODO: fix bug", &kw()) > compute_salience("no urgency here", &kw())
        );
    }

    #[test]
    fn test_compute_salience_decision_language() {
        let s = compute_salience(
            "DECISION: Chose Tokio over async-std because broader ecosystem",
            &kw(),
        );
        assert!(s >= 0.3, "decision text should score high, got {}", s);
    }

    #[test]
    fn test_compute_salience_decision_with_rationale_boost() {
        let without = compute_salience("Decided to use Redis", &kw());
        let with = compute_salience("Decided to use Redis because it has better pub/sub", &kw());
        assert!(
            with > without,
            "rationale should boost: {} vs {}",
            with,
            without
        );
    }

    #[test]
    fn test_compute_salience_bug_language() {
        let s = compute_salience("Bug: the server crashes on empty input", &kw());
        assert!(s >= 0.4, "bug text should score high, got {}", s);
    }

    #[test]
    fn test_compute_salience_blocker() {
        let s = compute_salience("BLOCKER: blocked on API key provisioning", &kw());
        assert!(s >= 0.3, "blocker text should score high, got {}", s);
    }

    #[test]
    fn test_compute_salience_architecture() {
        let s = compute_salience("The system boundary separates intake from storage", &kw());
        assert!(
            (0.15..0.3).contains(&s),
            "generic structure should score moderately, got {}",
            s
        );
    }

    #[test]
    fn test_compute_salience_preference() {
        let s = compute_salience("User prefers dark mode, always use minimal UI", &kw());
        assert!(s >= 0.3, "preference text should score, got {}", s);
    }

    #[test]
    fn test_compute_salience_minimum_floor() {
        let s = compute_salience("nothing special here at all", &kw());
        assert!(s >= 0.05, "floor should be 0.05, got {}", s);
    }

    #[test]
    fn test_compute_salience_capped_at_one() {
        let s = compute_salience("DECISION: Chose X because crash bug regression BLOCKER TODO architecture API schema module user prefers convention fn main() {} ``` code ```", &kw());
        assert!(s <= 1.0, "should cap at 1.0, got {}", s);
    }
}
