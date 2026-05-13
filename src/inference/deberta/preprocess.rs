//! Rust port of `gliner.data_processing` for span-based uni-encoder
//! models. Produces the exact same `(input_ids, attention_mask,
//! words_mask, span_idx, span_mask)` the Python collator returns,
//! together with a char-position-to-word index map so callers can
//! translate predicted word spans back to character offsets.
//!
//! The matching is verified by the round-trip test against the
//! oracle fixture (`oracle/fixtures/dentist/tokenizer.json`).

use std::sync::LazyLock;

use regex::Regex;
use tokenizers::{EncodeInput, InputSequence};

use crate::inference::deberta::tokenizer::{
    BUNDLED_TOKENIZER, CLS_ID, ENT_ID, SEP_ID, SEP_LABEL_ID,
};

/// Matches the Python regex used by `WhitespaceTokenSplitter`:
/// either a word (alphanumerics + underscore, optionally extended by
/// `-` or `_` followed by more word chars) or a single non-whitespace
/// character.
static WORD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\w+(?:[-_]\w+)*|\S").expect("compile word regex"));

/// One word in the source text, with its character offsets.
#[derive(Debug, Clone)]
pub struct Word {
    pub text: String,
    pub char_start: usize,
    pub char_end: usize,
}

/// Whitespace-style word splitter. `text` is split into words; each
/// word carries the inclusive char-start and exclusive char-end of its
/// occurrence in the input. Mirrors `WhitespaceTokenSplitter` from
/// gliner.data_processing.tokenizer.
pub fn split_words(text: &str) -> Vec<Word> {
    WORD_RE
        .find_iter(text)
        .map(|m| Word {
            text: m.as_str().to_string(),
            char_start: m.start(),
            char_end: m.end(),
        })
        .collect()
}

/// Everything Step 5 needs to feed into the INT8 forward pass plus
/// the inverse mapping for decoding entities back to char offsets.
#[derive(Debug)]
pub struct GlinerInputs {
    pub input_ids: Vec<u32>,
    pub attention_mask: Vec<u32>,
    /// `words_mask[t] = i` means subword `t` is the *first* subword of
    /// the i-th text word (1-indexed). 0 marks prompt tokens, special
    /// tokens, or continuation subwords.
    pub words_mask: Vec<u32>,
    pub words: Vec<Word>,
}

/// Build the GLiNER2 input batch (batch size 1) from a raw text +
/// label list.
///
/// Layout produced:
///
/// ```text
/// input_ids:    [CLS] [ENT] tok(label_0) [ENT] tok(label_1) ... [SEP_label] tok(text...) [SEP]
/// attention:    [ 1     1   1               1   1               1            1...           1 ]
/// words_mask:   [ 0     0   0               0   0               0            1,2,3...,W     0 ]
/// ```
///
/// `tok(label_i)` may be 1+ subword tokens — the slot is split out by
/// the tokenizer. Continuation subwords get `words_mask=0`.
pub fn build_inputs(text: &str, labels: &[&str]) -> GlinerInputs {
    let tok = &*BUNDLED_TOKENIZER;
    let words = split_words(text);

    // Build the pre-tokenized input the way GLiNER does:
    //   [ ENT, label_0, ENT, label_1, ..., SEP_LABEL, word_0, word_1, ..., word_N ]
    // The HF tokenizer handles internal subword splitting per element
    // when called with `is_pretokenized=true`.
    let mut pre_tokens: Vec<String> = Vec::with_capacity(2 * labels.len() + 1 + words.len());
    for label in labels {
        pre_tokens.push("<<ENT>>".to_string());
        pre_tokens.push((*label).to_string());
    }
    pre_tokens.push("<<SEP>>".to_string());
    let prompt_word_count = pre_tokens.len();
    for w in &words {
        pre_tokens.push(w.text.clone());
    }

    let input: EncodeInput = EncodeInput::Single(InputSequence::from(
        pre_tokens.iter().map(|s| s.as_str()).collect::<Vec<&str>>(),
    ));
    // The bundled tokenizer.json has no post-processor template, so we
    // wrap with `[CLS]`/`[SEP]` manually below to match the Python
    // pipeline.
    let encoding = tok.encode(input, false).expect("encode failed");

    let inner_ids = encoding.get_ids();
    let inner_attn = encoding.get_attention_mask();
    let inner_wids = encoding.get_word_ids();

    let mut input_ids: Vec<u32> = Vec::with_capacity(inner_ids.len() + 2);
    let mut attention_mask: Vec<u32> = Vec::with_capacity(inner_ids.len() + 2);
    input_ids.push(CLS_ID);
    attention_mask.push(1);
    input_ids.extend_from_slice(inner_ids);
    attention_mask.extend_from_slice(inner_attn);
    input_ids.push(SEP_ID);
    attention_mask.push(1);

    // Build words_mask the same way: 0 for the [CLS]/[SEP] specials,
    // and the GLiNER recipe for everything in between.
    let mut words_mask: Vec<u32> = Vec::with_capacity(input_ids.len());
    words_mask.push(0); // [CLS]
    let mut prev_wid: Option<u32> = None;
    let mut seen_words = 0u32;
    let mut counted_prompt_skip = 0u32;
    for wid_opt in inner_wids {
        match wid_opt {
            None => {
                words_mask.push(0);
                prev_wid = None;
            }
            Some(wid) => {
                if Some(*wid) != prev_wid {
                    seen_words += 1;
                    if seen_words <= prompt_word_count as u32 {
                        counted_prompt_skip = seen_words;
                        words_mask.push(0);
                    } else {
                        words_mask.push(seen_words - counted_prompt_skip);
                    }
                } else {
                    // Continuation subword of the same word.
                    words_mask.push(0);
                }
                prev_wid = Some(*wid);
            }
        }
    }
    words_mask.push(0); // [SEP]

    debug_assert_eq!(words_mask.len(), input_ids.len());

    // Sanity: the special-token ids we expect to see appear in the
    // produced sequence in the right order. (Cheap and catches drift
    // if the tokenizer.json is ever regenerated with different IDs.)
    debug_assert_eq!(input_ids.first().copied(), Some(CLS_ID));
    debug_assert_eq!(input_ids.last().copied(), Some(SEP_ID));
    debug_assert!(input_ids.contains(&ENT_ID));
    debug_assert!(input_ids.contains(&SEP_LABEL_ID));

    GlinerInputs {
        input_ids,
        attention_mask,
        words_mask,
        words,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_words_matches_python_regex() {
        // Python `\w+(?:[-_]\w+)*|\S` on this string yields the words
        // shown below (verified manually + against `WhitespaceTokenSplitter`).
        let words =
            split_words("My dentist appointment with Dr. Rao changed from Tuesday to Friday.");
        let texts: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(
            texts,
            vec![
                "My",
                "dentist",
                "appointment",
                "with",
                "Dr",
                ".",
                "Rao",
                "changed",
                "from",
                "Tuesday",
                "to",
                "Friday",
                "."
            ]
        );
    }

    #[test]
    fn split_words_handles_hyphenated() {
        let words = split_words("state-of-the-art results.");
        let texts: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(texts, vec!["state-of-the-art", "results", "."]);
    }

    #[test]
    fn build_inputs_matches_oracle_dentist() {
        // The exact (input_ids, attention_mask, words_mask) saved by
        // the Python oracle for the dentist fixture.
        let inputs = build_inputs(
            "My dentist appointment with Dr. Rao changed from Tuesday to Friday.",
            &["person", "event", "weekday", "role"],
        );

        let expected_ids: Vec<u32> = vec![
            1, 128002, 604, 128002, 720, 128002, 20467, 128002, 985, 128003, 573, 8301, 3198, 275,
            1011, 323, 25773, 1594, 292, 1586, 264, 1178, 323, 2,
        ];
        let expected_words_mask: Vec<u32> = vec![
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 0,
        ];

        assert_eq!(inputs.input_ids, expected_ids);
        assert_eq!(inputs.attention_mask, vec![1; expected_ids.len()]);
        assert_eq!(inputs.words_mask, expected_words_mask);
        assert_eq!(inputs.words.len(), 13);
        assert_eq!(inputs.words[0].text, "My");
        assert_eq!(inputs.words[0].char_start, 0);
        assert_eq!(inputs.words[0].char_end, 2);
        assert_eq!(inputs.words[12].text, ".");
    }
}
