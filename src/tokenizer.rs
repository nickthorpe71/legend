use std::sync::LazyLock;

use tokenizers::Tokenizer;

/// Raw bytes of the bundled all-MiniLM-L6-v2 tokenizer config. Single
/// source of truth — `embed.rs` and Step 3 both build their tokenizers
/// from this constant so a model swap only changes the path here.
pub const TOKENIZER_BYTES: &[u8] = include_bytes!("../models/all-MiniLM-L6-v2-q/tokenizer.json");

// Untruncated, unpadded tokenizer used by Step 3 for true token counting
// and for round-tripping ids ↔ text on the long-input window split. The
// bundled `tokenizer.json` ships with `truncation: max_length=128` and
// `padding: Fixed(128)` baked in — both must be cleared, or every input
// reports as 128 tokens (padding inflates the count, truncation hides
// inputs that exceed 512). `embed.rs` builds its own instance from the
// same bytes with truncation set to the model's 512 limit.
static TOKENIZER: LazyLock<Tokenizer> = LazyLock::new(|| {
    let mut tokenizer =
        Tokenizer::from_bytes(TOKENIZER_BYTES).expect("Failed to load embedded tokenizer.");
    tokenizer.with_truncation(None).expect("clear truncation");
    tokenizer.with_padding(None);
    tokenizer
});

/// True token count for `text` against the bundled MiniLM tokenizer.
/// Step 3 reads this to decide whether the input fits in one window
/// (≤480) or needs to be split.
pub fn token_count(text: &str) -> usize {
    let encoding = TOKENIZER.encode(text, true).expect("tokenization failed");
    encoding.get_ids().len()
}

/// Encode `text` to its full token id sequence. Step 3 uses this on the
/// long-input branch to slice `chunks(480)` and decode each piece back
/// to a per-window text.
pub fn encode_ids(text: &str) -> Vec<u32> {
    let encoding = TOKENIZER.encode(text, true).expect("tokenization failed");
    encoding.get_ids().to_vec()
}

/// Decode a token id slice back to text, dropping CLS/SEP/etc. Lossy
/// for whitespace and casing — acceptable for v0's token-budget long
/// path; SaT will replace this with char-offset slicing of the original
/// text and the round-trip goes away.
pub fn decode(ids: &[u32]) -> String {
    TOKENIZER.decode(ids, true).expect("decode failed")
}
