/// Extractive summarization utilities.

use crate::memory::ShortTermEntry;

const MAX_SUMMARY_LEN: usize = 200;
const MAX_GROUP_SUMMARY_LEN: usize = 300;
const CHUNK_TARGET_LEN: usize = 200;

/// Decision-rationale keywords that boost sentence importance.
const DECISION_KEYWORDS: &[&str] = &[
    "because", "chose", "decided", "instead", "rather", "reason",
    "tradeoff", "trade-off", "over", "prefer", "picked", "approach",
    "why", "so that", "in order to",
];

/// Summarize a single text chunk (extractive — pick best sentence).
/// Prioritizes sentences with code references and decision rationale.
pub fn summarize_single(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.len() <= MAX_SUMMARY_LEN {
        return trimmed.to_string();
    }

    let sentences: Vec<&str> = trimmed
        .split(|c: char| c == '.' || c == '!' || c == '?' || c == '\n')
        .map(|s| s.trim())
        .filter(|s| s.len() > 10)
        .collect();

    if sentences.is_empty() {
        return trimmed.chars().take(MAX_SUMMARY_LEN).collect();
    }

    let best = sentences
        .iter()
        .max_by_key(|s| {
            let lower = s.to_lowercase();
            let words = s.split_whitespace().count();
            let has_code = s.contains("fn ") || s.contains("struct ") || s.contains("impl ");
            let has_key = s.contains(':') || s.contains('=') || s.contains("TODO");
            let has_decision = DECISION_KEYWORDS.iter().any(|kw| lower.contains(kw));
            words + if has_code { 5 } else { 0 }
                 + if has_key { 3 } else { 0 }
                 + if has_decision { 8 } else { 0 }
        })
        .unwrap_or(&sentences[0]);

    if best.len() <= MAX_SUMMARY_LEN { best.to_string() } else { best.chars().take(MAX_SUMMARY_LEN).collect() }
}

/// Merge two texts and pick the best sentence.
pub fn summarize_text(existing: &str, incoming: &str) -> String {
    summarize_single(&format!("{} {}", existing, incoming))
}

/// Summarize a group of short-term entries (pick top 3 by salience+usage).
pub fn summarize_group(group: &[ShortTermEntry]) -> String {
    let mut sorted: Vec<&ShortTermEntry> = group.iter().collect();
    sorted.sort_by(|a, b| {
        let score_a = a.salience + a.usage as f32 * 0.1;
        let score_b = b.salience + b.usage as f32 * 0.1;
        score_b.partial_cmp(&score_a).unwrap()
    });

    let mut combined = String::new();
    for entry in sorted.iter().take(3) {
        if !combined.is_empty() {
            combined.push_str(" | ");
        }
        let summary = if entry.summary.is_empty() {
            summarize_single(&entry.text)
        } else {
            entry.summary.clone()
        };
        combined.push_str(&summary);
    }

    if combined.is_empty() {
        "Consolidated memory".to_string()
    } else if combined.len() > MAX_GROUP_SUMMARY_LEN {
        combined.chars().take(MAX_GROUP_SUMMARY_LEN).collect()
    } else {
        combined
    }
}

/// Split text into chunks of roughly CHUNK_TARGET_LEN chars.
pub fn chunk_text(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();

    for line in text.lines() {
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(line.trim());
        if current.len() > CHUNK_TARGET_LEN {
            chunks.push(current.clone());
            current.clear();
        }
    }

    if !current.trim().is_empty() {
        chunks.push(current);
    }

    if chunks.is_empty() { vec![text.trim().to_string()] } else { chunks }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_summarize_single_short() {
        assert_eq!(summarize_single("short text"), "short text");
    }

    #[test]
    fn test_summarize_single_long() {
        let text = "This is a long sentence about memory systems. fn handle_memory() is the main handler. It processes incoming ticks and queries. The system uses cosine similarity for matching.";
        let summary = summarize_single(text);
        assert!(summary.len() <= 200);
    }

    #[test]
    fn test_chunk_text_short() {
        let chunks = chunk_text("short text");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "short text");
    }

    #[test]
    fn test_chunk_text_long() {
        let lines: Vec<String> = (0..20).map(|i| format!("Line {} has some content here.", i)).collect();
        let text = lines.join("\n");
        let chunks = chunk_text(&text);
        assert!(chunks.len() > 1);
    }

    #[test]
    fn test_decision_rationale_boosted() {
        let text = "This module handles memory operations with buffers and caches. \
                     We chose cosine similarity over Euclidean distance because it is scale-invariant. \
                     The system also supports batch processing for throughput optimization.";
        let summary = summarize_single(text);
        assert!(summary.contains("chose") || summary.contains("because"),
            "Decision-rationale sentence should be selected, got: {}", summary);
    }
}
