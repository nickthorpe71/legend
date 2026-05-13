//! Thin wrapper around `tokenizers::Tokenizer` for the GLiNER2
//! tokenizer (DeBERTa-v3 SentencePiece + GLiNER's 4 added special
//! tokens). The `tokenizer.json` lives at
//! `models/gliner2-tokenizer/tokenizer.json` and is bundled at build
//! time so the binary is self-contained.
//!
//! Phase 2 scope is just load + decode round-trip. The full
//! prompt-prepending + word-splitter pipeline (matching
//! `gliner.data_processor`) is deferred to Phase 7. Until then the
//! per-layer encoder validation runs against the oracle's
//! pre-computed `input_ids`.

use std::sync::LazyLock;
use tokenizers::Tokenizer;

const TOKENIZER_BYTES: &[u8] = include_bytes!("../../../models/gliner2-tokenizer/tokenizer.json");

pub static BUNDLED_TOKENIZER: LazyLock<Tokenizer> = LazyLock::new(|| {
    Tokenizer::from_bytes(TOKENIZER_BYTES).expect("failed to parse bundled GLiNER2 tokenizer")
});

/// Special-token IDs we rely on by value at the moment. Anchored to
/// the GLiNER2 added vocab — if these drift the loader will catch it
/// via the assertions in `weights.rs`.
pub const CLS_ID: u32 = 1;
pub const SEP_ID: u32 = 2;
pub const ENT_ID: u32 = 128_002;
pub const SEP_LABEL_ID: u32 = 128_003;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizer_loads_and_has_expected_vocab() {
        let tok = &*BUNDLED_TOKENIZER;
        // GLiNER2 added 4 specials on top of DeBERTa-v3's 128 000.
        assert_eq!(tok.get_vocab_size(true), 128_004);
        assert_eq!(tok.token_to_id("<<ENT>>"), Some(ENT_ID));
        assert_eq!(tok.token_to_id("<<SEP>>"), Some(SEP_LABEL_ID));
        assert_eq!(tok.token_to_id("[CLS]"), Some(CLS_ID));
    }

    #[test]
    fn tokenizes_text_to_oracle_ids() {
        // Subset of the oracle's tokenizer.json — text words only,
        // verifying our tokenizer agrees on the BPE split. We
        // tokenize the raw text without specials, then compare.
        let tok = &*BUNDLED_TOKENIZER;
        let enc = tok
            .encode("My dentist appointment with Dr. Rao", false)
            .expect("encode");
        // From oracle: ['My', 'dentist', 'appointment', 'with', 'Dr', '.', 'Rao']
        // tokens -> [573, 8301, 3198, 275, 1011, ?, 25773] (the '.' token id
        // varies with whether '.' is its own token or attaches to 'Dr').
        // Just spot-check the unambiguous starts.
        let ids = enc.get_ids();
        assert_eq!(ids[0], 573, "got {:?}", ids);
        assert_eq!(ids[1], 8301);
        assert_eq!(ids[2], 3198);
    }
}
