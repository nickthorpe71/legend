use crate::tokenizer;
use crate::types::{Input, Window};

/// Per-window token budget. GLiNER2 (the §11.7 extractor) accepts up to
/// ~512 tokens per call; the 480 ceiling leaves margin for special
/// tokens, positional buffer, and any coref-context bytes the extractor
/// consumes from the window's edges.
pub const WINDOW_TOKEN_BUDGET: usize = 480;

/// Step 3 of the tick pipeline. Chunk `input.text` into one or more
/// windows sized to fit the per-window token budget.
///
/// Two paths by length:
/// - **Short (≤480 tokens, the common case for chat-message-sized
///   ticks).** No segmentation; the whole input is one window.
/// - **Long (>480 tokens).** Token-budget split: encode to ids, slice
///   `chunks(WINDOW_TOKEN_BUDGET)`, decode each chunk back to text.
///   Lossy on whitespace/casing — this is v0's placeholder for SaT
///   (Segment Any Text), which will replace the round-trip with
///   sentence-aware char-offset slicing of the original text.
///
/// Pre-splitting at sub-window granularity is deliberately avoided —
/// GLiNER2 finds cross-sentence relations within a window, and forcing
/// an internal split would risk separating an entity from its relation
/// partner.
pub fn window_input(input: &Input) -> Vec<Window> {
    if input.text.trim().is_empty() {
        return vec![Window {
            text: String::new(),
            token_count: 0,
        }];
    }

    let token_count = tokenizer::token_count(&input.text);
    if token_count <= WINDOW_TOKEN_BUDGET {
        return vec![Window {
            text: input.text.clone(),
            token_count,
        }];
    }

    // TODO: SaT — replace token-budget split with SaT-derived sentence
    // boundaries + greedy grouping (§11.4). The fallback to token-budget
    // windowing stays for individual SaT segments that themselves
    // exceed the budget.
    let ids = tokenizer::encode_ids(&input.text);
    ids.chunks(WINDOW_TOKEN_BUDGET)
        .map(|chunk| Window {
            text: tokenizer::decode(chunk),
            token_count: chunk.len(),
        })
        .collect()
}
