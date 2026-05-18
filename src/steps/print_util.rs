//! Shared formatting helpers for the per-step `print_stepN` debug
//! printers. Step-specific output stays in each step's module; only
//! generic primitives live here.

/// Truncate `s` to `max` characters, appending `…` when shortened.
/// Char-aware so multi-byte input doesn't slice mid-codepoint.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}
