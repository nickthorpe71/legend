//! Token-level void filter. Classifies each whitespace-and-slash atom
//! in the input as either content-bearing (caller should propagate)
//! or void-filler (caller should drop).
//!
//! The decision is a name lookup against the hypergraph's `by_name`
//! index: a token whose lowercase surface form resolves to any
//! Element with `polarity == Polarity::Void` is filler. The 118
//! closed-class stop words seeded under the VOID anchor cover the
//! common cases ("the", "of", "and", "is", ...); the long tail
//! (rare prepositions, dialectal variants, neologistic fillers) is
//! a follow-on once an empirical leak rate is measurable.
//!
//! Why name-lookup instead of cosine-against-prototype: MiniLM
//! single-word embeddings are degenerate (the model is trained for
//! sentence-level contrast), and the seeded void elements give us
//! ~95% coverage of English function-word tokens for free. A proper
//! per-token contextual embedding extraction is a tracked follow-on
//! task (phase 5 in the void-tree rewrite plan, deferred).

use crate::tick_pipeline::orthographic::{ChunkScale, OrthographicChunk, collect_raw_tokens};
use crate::types::{Hypergraph, Polarity};

/// Walk every raw token in `text` and emit a `Token`-scale
/// `OrthographicChunk` for each one whose lowercase surface form
/// does *not* resolve to a `Polarity::Void` element. Function words
/// disappear; content tokens survive.
pub fn extract_content_tokens(text: &str, hypergraph: &Hypergraph) -> Vec<OrthographicChunk> {
    let mut out = Vec::new();
    let mut seen_text: std::collections::HashSet<String> = std::collections::HashSet::new();
    for t in collect_raw_tokens(text) {
        if is_void_token(&t.text, hypergraph) {
            continue;
        }
        if !seen_text.insert(t.text.clone()) {
            continue;
        }
        out.push(OrthographicChunk {
            text: t.text,
            char_start: t.char_start,
            char_end: t.char_end,
            scale: ChunkScale::Token,
        });
    }
    out
}

/// True iff `token` resolves (case-insensitively) to any Element with
/// `polarity == Polarity::Void`. Tries the exact surface form first
/// (catches case-sensitive entries like "I"), then a lowercased
/// fallback (catches "The" / "the" / "THE" uniformly).
pub fn is_void_token(token: &str, hypergraph: &Hypergraph) -> bool {
    if resolves_to_void(token, hypergraph) {
        return true;
    }
    let lower = token.to_lowercase();
    if lower != token && resolves_to_void(&lower, hypergraph) {
        return true;
    }
    false
}

fn resolves_to_void(name: &str, hypergraph: &Hypergraph) -> bool {
    let Some(ids) = hypergraph.by_name.get(name) else {
        return false;
    };
    ids.iter()
        .any(|id| hypergraph.elements[id.0 as usize].polarity == Polarity::Void)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seed::load_seed_graph;

    #[test]
    fn common_stop_words_classify_as_void() {
        let hypergraph = load_seed_graph();
        for word in ["the", "of", "and", "if", "is", "to", "in", "on"] {
            assert!(
                is_void_token(word, &hypergraph),
                "expected '{word}' to classify as void"
            );
        }
    }

    #[test]
    fn capitalized_stop_words_still_classify_as_void() {
        let hypergraph = load_seed_graph();
        for word in ["The", "Of", "AND", "If"] {
            assert!(
                is_void_token(word, &hypergraph),
                "expected '{word}' to classify as void (case-insensitive)"
            );
        }
    }

    #[test]
    fn content_words_do_not_classify_as_void() {
        let hypergraph = load_seed_graph();
        for word in ["London", "appointment", "dentist", "Sarah", "Tuesday"] {
            assert!(
                !is_void_token(word, &hypergraph),
                "did not expect '{word}' to classify as void"
            );
        }
    }

    #[test]
    fn pathological_input_keeps_only_content_tokens() {
        let hypergraph = load_seed_graph();
        let text = "I need to make sure I wait outside the gates of london town";
        let chunks = extract_content_tokens(text, &hypergraph);
        let texts: Vec<&str> = chunks.iter().map(|c| c.text.as_str()).collect();

        // Function-word stragglers must be gone:
        for filler in ["I", "to", "the", "of"] {
            assert!(
                !texts.contains(&filler),
                "void token '{filler}' leaked through: {texts:?}"
            );
        }
        // Content tokens must survive:
        for content in ["need", "make", "sure", "wait", "gates", "london", "town"] {
            assert!(
                texts.contains(&content),
                "content token '{content}' missing: {texts:?}"
            );
        }
    }

    #[test]
    fn empty_input_yields_no_chunks() {
        let hypergraph = load_seed_graph();
        assert!(extract_content_tokens("", &hypergraph).is_empty());
        assert!(extract_content_tokens("   ", &hypergraph).is_empty());
    }

    #[test]
    fn token_chunks_offsets_round_trip_into_text() {
        let hypergraph = load_seed_graph();
        let text = "The dentist called Sarah on Tuesday.";
        let chunks = extract_content_tokens(text, &hypergraph);
        for c in &chunks {
            let slice = &text[c.char_start..c.char_end];
            assert_eq!(slice, c.text, "offset mismatch for {c:?}");
        }
    }
}
