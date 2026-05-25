//! Temporal-span extractor for `run_extractors`. Pure regex over the input text,
//! no model call. v0 covers the high-frequency cases:
//!
//! - **Weekdays** — `Monday`..`Sunday`, case-insensitive.
//! - **Months** — `January`..`December`, case-insensitive.
//! - **Common phrases** — `today`, `tomorrow`, `yesterday`, `tonight`.
//!
//! Each match becomes an `(span, instance_of, "weekday" | "month" |
//! "time")` extraction proposal. The §11.7 spec mentions
//! `chrono-english` for relative-phrase parsing — that's a deferred
//! extension once we need to turn `"next Tuesday"` into a concrete
//! datetime. For `run_extractors`'s job (proposing candidate temporal spans),
//! the regex pass is sufficient and matches the doc's listed kinds.

use std::sync::LazyLock;

use regex::Regex;

#[derive(Debug, Clone)]
pub struct TemporalSpan {
    pub char_start: usize,
    pub char_end: usize,
    pub text: String,
    /// One of `"weekday"`, `"month"`, `"time"`.
    pub kind: &'static str,
}

static WEEKDAY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:Monday|Tuesday|Wednesday|Thursday|Friday|Saturday|Sunday)\b")
        .expect("compile weekday regex")
});

static MONTH_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(?:January|February|March|April|May|June|July|August|September|October|November|December)\b",
    )
    .expect("compile month regex")
});

static PHRASE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:today|tomorrow|yesterday|tonight)\b").expect("compile phrase regex")
});

/// Find every temporal span in `text`. Spans are returned in char-order;
/// overlaps are not produced because each regex matches disjoint
/// alternatives.
pub fn extract_temporal(text: &str) -> Vec<TemporalSpan> {
    let mut out: Vec<TemporalSpan> = Vec::new();
    for m in WEEKDAY_RE.find_iter(text) {
        out.push(TemporalSpan {
            char_start: m.start(),
            char_end: m.end(),
            text: m.as_str().to_string(),
            kind: "weekday",
        });
    }
    for m in MONTH_RE.find_iter(text) {
        out.push(TemporalSpan {
            char_start: m.start(),
            char_end: m.end(),
            text: m.as_str().to_string(),
            kind: "month",
        });
    }
    for m in PHRASE_RE.find_iter(text) {
        out.push(TemporalSpan {
            char_start: m.start(),
            char_end: m.end(),
            text: m.as_str().to_string(),
            kind: "time",
        });
    }
    out.sort_by_key(|s| s.char_start);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_weekday_and_phrase() {
        let spans = extract_temporal("We'll meet on Tuesday or maybe tomorrow.");
        let kinds: Vec<(&str, &str)> = spans.iter().map(|s| (s.text.as_str(), s.kind)).collect();
        assert_eq!(kinds, vec![("Tuesday", "weekday"), ("tomorrow", "time")]);
    }

    #[test]
    fn finds_month() {
        let spans = extract_temporal("Launching in October.");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "October");
        assert_eq!(spans[0].kind, "month");
    }

    #[test]
    fn ignores_non_temporal() {
        let spans = extract_temporal("Pure text with no times in it.");
        assert!(spans.is_empty());
    }
}
