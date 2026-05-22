//! SVO (subject-verb-object) pattern extractor.
//!
//! Surface-pattern OpenIE: for each `Phrase` chunk produced by the
//! orthographic + void-filter pass, find tokens whose surface form
//! looks like a verb (irregular-verb lexicon or suffix morphology),
//! then emit one `(subject, attribute_text, object)` triple per verb.
//!
//! Subject is sticky across verbs in the same phrase — "Sarah works
//! at X and lives in Y" attaches both predicates to Sarah. Personal
//! pronouns that the void filter dropped (e.g. "I") are recovered
//! from the raw input via [`find_pronoun_subject`].
//!
//! Output: every triple is `Defeasible`-bound. Step 8 will resolve
//! `attribute_text` to an attribute-name Element (lookup → embedding
//! search → mint Defeasible if no match). Replay confirms or prunes.
//!
//! ## What this catches
//!
//! - "Sarah lives in Paris" → (Sarah, "lives in", Paris)
//! - "John works at Google" → (John, "works at", Google)
//! - "I got a Samsung Galaxy S22" → (I, "got a", Samsung Galaxy S22)
//! - "Sarah works at X and lives in Y" → two triples, both subject=Sarah
//!
//! ## What this misses (acceptable; defer to replay or known branch)
//!
//! - Copular sentences ("Sarah is happy"): `is` is void; remaining
//!   tokens are Sarah + happy with no verb-shape between them.
//! - Sentences without a Phrase boundary: bound by the Phrase pass.

use crate::steps::orthographic::{ChunkScale, OrthographicChunk};
use crate::steps::relation_patterns::{
    DEFAULT_SURFACE_CONFIDENCE, ObjectRef, PatternSource, RelationCandidate,
};
use crate::types::RelationStatus;

/// Extract candidate SVO triples from a phrase + token chunk slice.
///
/// `chunks` must contain both `Phrase`-scale and `Token`-scale entries
/// for the same input; this function filters internally.
pub fn extract_svo_triples(text: &str, chunks: &[OrthographicChunk]) -> Vec<RelationCandidate> {
    let mut out = Vec::new();

    // Multi-word proper-noun runs ("British Broadcasting Corporation",
    // "Lockheed Martin", "Engineering News"). Tokens inside these are
    // entity tokens — even if their surface form ends in -ed/-ing/-en
    // they're not verbs. We use these ranges to:
    //   1. Skip iterating a proper-noun-run as if it were a clause —
    //      otherwise "British Broadcasting Corporation" gets parsed
    //      as `(British) — Broadcasting → (Corporation)` because
    //      "Broadcasting" matches the -ing verb suffix.
    //   2. Exclude such tokens from being verb candidates within
    //      a containing clause phrase — otherwise "X is employed by
    //      British Broadcasting Corporation" splits the object span
    //      at "Broadcasting".
    let pn_runs: Vec<(usize, usize)> = crate::steps::orthographic::extract_proper_noun_runs(text)
        .into_iter()
        .map(|c| (c.char_start, c.char_end))
        .collect();
    let in_pn_run =
        |start: usize, end: usize| -> bool { pn_runs.iter().any(|&(s, e)| s <= start && end <= e) };

    for phrase in chunks.iter().filter(|c| c.scale == ChunkScale::Phrase) {
        // Skip phrases that are themselves a proper-noun run —
        // those are entities, not clauses.
        if in_pn_run(phrase.char_start, phrase.char_end) {
            continue;
        }

        let tokens: Vec<&OrthographicChunk> = chunks
            .iter()
            .filter(|c| {
                c.scale == ChunkScale::Token
                    && c.char_start >= phrase.char_start
                    && c.char_end <= phrase.char_end
            })
            .collect();

        if tokens.len() < 3 {
            continue;
        }

        let verb_idxs: Vec<usize> = tokens
            .iter()
            .enumerate()
            .filter(|(_, t)| is_verb_shape(&t.text) && !in_pn_run(t.char_start, t.char_end))
            .map(|(i, _)| i)
            .collect();

        if verb_idxs.is_empty() {
            continue;
        }

        // Subject preference order:
        //
        //  1. If the raw input between the phrase's start and the
        //     first content token contains a personal pronoun
        //     ("I", "He", "She", "They", ...), use that as the
        //     subject. Pronouns are seeded as Void elements (so
        //     they don't pollute retrieval) and therefore filtered
        //     out by `extract_content_tokens` — but they're the
        //     natural syntactic subject. Adverbial modifiers
        //     ("recently", "really") between the pronoun and the
        //     verb should NOT win the subject slot.
        //  2. Otherwise, fall back to the sticky-content-token
        //     rule: subject = content tokens before the first
        //     verb-shape token. Preserves coordination
        //     ("Sarah works at X and lives in Y" → both Sarah).
        //  3. If neither yields anything (phrase starts with verb
        //     and no pronoun precedes), skip.
        let subj_end_idx = verb_idxs[0];
        let preamble_end = tokens
            .first()
            .map(|t| t.char_start)
            .unwrap_or(phrase.char_end);
        let pronoun = find_pronoun_subject(text, phrase.char_start, preamble_end);
        let (subj_first_start, subj_last_end, subj_pronoun_end) =
            if let Some((p_start, p_end)) = pronoun {
                (p_start, p_end, Some(p_end))
            } else if subj_end_idx == 0 {
                continue;
            } else {
                (
                    tokens[0].char_start,
                    tokens[subj_end_idx - 1].char_end,
                    None,
                )
            };

        for (k, &vi) in verb_idxs.iter().enumerate() {
            let obj_start_idx = vi + 1;
            let obj_end_idx = if k + 1 < verb_idxs.len() {
                verb_idxs[k + 1]
            } else {
                tokens.len()
            };

            if obj_start_idx >= obj_end_idx {
                continue;
            }

            let obj_first = tokens[obj_start_idx];
            let obj_last = tokens[obj_end_idx - 1];

            // The attribute starts right after the token immediately
            // before this verb — that's the subject's tail for k=0
            // and the *previous object's* tail for k>0. When the
            // subject is a synthesized pronoun, anchor the attribute
            // span at the pronoun's end so adverbial modifiers between
            // the pronoun and verb ("recently" in "I recently got X")
            // land in the attribute text instead of dangling.
            let prev_tail_end = if k == 0 {
                subj_pronoun_end.unwrap_or(subj_last_end)
            } else {
                tokens[vi - 1].char_end
            };
            let attr_text = text[prev_tail_end..obj_first.char_start].trim().to_string();
            if attr_text.is_empty() {
                continue;
            }

            out.push(RelationCandidate {
                source: PatternSource::Svo,
                subject_char_start: subj_first_start,
                subject_char_end: subj_last_end,
                attribute_name: attr_text,
                object: ObjectRef::Span {
                    char_start: obj_first.char_start,
                    char_end: obj_last.char_end,
                },
                confidence: DEFAULT_SURFACE_CONFIDENCE,
                status: RelationStatus::Defeasible,
                event_anchor: None,
            });
        }
    }

    out
}

/// Closed-class personal pronouns that can anchor a clause as
/// subject. We look for these in the raw input when the content-
/// token list starts with a verb — pronouns are seeded as Void
/// elements (filtered out of content tokens) but remain valid
/// syntactic subjects.
const SUBJECT_PRONOUNS: &[&str] = &["he", "i", "it", "she", "they", "we", "you"];

/// Search `text[start..end]` for a sentence-initial pronoun subject.
/// Returns the pronoun's `(char_start, char_end)` byte offsets in
/// the original text. `None` if the preamble carries no pronoun.
/// Case-insensitive.
fn find_pronoun_subject(text: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    let slice = &text[start..end];
    let mut cursor = 0usize;
    for raw in slice.split(|c: char| c.is_whitespace()) {
        let trimmed = raw.trim_matches(|c: char| !c.is_alphanumeric());
        if !trimmed.is_empty() && SUBJECT_PRONOUNS.contains(&trimmed.to_ascii_lowercase().as_str())
        {
            let tok_offset = slice[cursor..].find(trimmed)?;
            let tok_start = start + cursor + tok_offset;
            let tok_end = tok_start + trimmed.len();
            return Some((tok_start, tok_end));
        }
        cursor += raw.len() + 1;
    }
    None
}

/// High-frequency irregular verbs that the suffix-only detector
/// misses. Past, present, and base forms of the 100-ish most common
/// English verbs that don't end in `-ed`/`-ing`/`-en`/`-s`. Hand-
/// curated from Zipf-frequency lists; favors verbs that change a
/// substrate's view of the world (acquire/transfer/move/state)
/// over modal auxiliaries.
///
/// Sorted; case-insensitive membership test runs in O(log N).
const IRREGULAR_VERBS: &[&str] = &[
    "am",
    "are",
    "ate",
    "be",
    "been",
    "began",
    "begin",
    "bit",
    "bite",
    "blew",
    "bore",
    "born",
    "bought",
    "break",
    "broke",
    "broken",
    "brought",
    "build",
    "built",
    "came",
    "caught",
    "choose",
    "chose",
    "chosen",
    "come",
    "cost",
    "cut",
    "did",
    "do",
    "does",
    "done",
    "drank",
    "drew",
    "drove",
    "eat",
    "feel",
    "fell",
    "felt",
    "find",
    "flew",
    "fly",
    "forgave",
    "forget",
    "forgive",
    "forgot",
    "fought",
    "found",
    "froze",
    "frozen",
    "gave",
    "get",
    "give",
    "go",
    "goes",
    "gone",
    "got",
    "grew",
    "grow",
    "had",
    "has",
    "have",
    "hear",
    "heard",
    "held",
    "hid",
    "hidden",
    "hide",
    "hit",
    "hold",
    "is",
    "kept",
    "knew",
    "know",
    "laid",
    "lay",
    "leave",
    "left",
    "lend",
    "lent",
    "let",
    "lit",
    "lost",
    "made",
    "make",
    "mean",
    "meant",
    "paid",
    "pay",
    "put",
    "quit",
    "ran",
    "rang",
    "read",
    "ride",
    "ring",
    "rise",
    "rode",
    "rose",
    "run",
    "said",
    "sang",
    "sank",
    "sat",
    "saw",
    "say",
    "see",
    "sell",
    "send",
    "sent",
    "set",
    "shine",
    "shone",
    "shoot",
    "shot",
    "shrank",
    "shut",
    "sing",
    "sink",
    "slept",
    "sold",
    "speak",
    "spend",
    "spent",
    "spoke",
    "stand",
    "stole",
    "stolen",
    "stood",
    "swam",
    "swear",
    "swim",
    "swore",
    "take",
    "taught",
    "tear",
    "tell",
    "thought",
    "threw",
    "throw",
    "told",
    "took",
    "tore",
    "torn",
    "understand",
    "understood",
    "was",
    "wear",
    "went",
    "were",
    "won",
    "wore",
    "worn",
    "wrote",
];

/// O(log N) case-insensitive membership probe.
fn is_irregular_verb(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    IRREGULAR_VERBS.binary_search(&lower.as_str()).is_ok()
}

/// Surface-form verb detector. Lexicon first (catches irregulars +
/// copular `is/am/are/was/were`); then suffix morphology.
pub(crate) fn is_verb_shape(token: &str) -> bool {
    if is_irregular_verb(token) {
        return true;
    }
    if token.chars().count() < 3 {
        return false;
    }
    let lower = token.to_lowercase();
    if lower.ends_with("ed") || lower.ends_with("ing") || lower.ends_with("en") {
        return true;
    }
    let first_char = token.chars().next().unwrap();
    if !first_char.is_uppercase()
        && lower.ends_with('s')
        && !lower.ends_with("ss")
        && !lower.ends_with("us")
    {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seed::load_seed_graph;
    use crate::steps::orthographic::extract_chunks;
    use crate::steps::void_filter::extract_content_tokens;

    fn chunks_for(text: &str) -> Vec<OrthographicChunk> {
        let hg = load_seed_graph();
        let mut chunks = extract_chunks(text);
        chunks.extend(extract_content_tokens(text, &hg));
        chunks.sort_by_key(|c| (c.char_start, c.char_end));
        chunks
    }

    fn obj_span(r: &RelationCandidate) -> (usize, usize) {
        match r.object {
            ObjectRef::Span {
                char_start,
                char_end,
            } => (char_start, char_end),
            ObjectRef::Label(_) => panic!("expected Span object, got Label"),
        }
    }

    #[test]
    fn irregular_verb_lexicon_is_sorted_and_dedup() {
        for pair in IRREGULAR_VERBS.windows(2) {
            assert!(
                pair[0] < pair[1],
                "lexicon not sorted: {:?} should sort before {:?}",
                pair[0],
                pair[1],
            );
        }
    }

    #[test]
    fn irregular_verbs_are_detected_as_verb_shape() {
        for v in [
            "got", "ran", "saw", "ate", "took", "made", "gave", "came", "went", "said", "had",
            "was", "were", "bought", "brought", "taught", "sent", "kept",
        ] {
            assert!(
                is_verb_shape(v),
                "`{v}` should be detected as a verb (irregular lexicon hit)"
            );
        }
    }

    #[test]
    fn capitalized_irregular_verbs_still_detected() {
        for v in ["Got", "Ran", "Bought", "WAS"] {
            assert!(is_verb_shape(v), "`{v}` (capitalized) should be detected");
        }
    }

    #[test]
    fn non_verbs_still_not_detected() {
        for w in ["the", "very", "big", "house", "phone"] {
            assert!(!is_verb_shape(w), "`{w}` shouldn't be a verb");
        }
    }

    #[test]
    fn got_phrase_forms_a_relation() {
        let text = "I got a Samsung Galaxy S22 from Best Buy on February 20th";
        let chunks = chunks_for(text);
        let rels = extract_svo_triples(text, &chunks);
        assert!(
            !rels.is_empty(),
            "expected ≥1 triple from `I got a Samsung Galaxy S22 …`, got 0"
        );
        let r = &rels[0];
        assert_eq!(&text[r.subject_char_start..r.subject_char_end], "I");
        assert!(
            r.attribute_name.contains("got"),
            "attribute should include `got`; got {:?}",
            r.attribute_name
        );
    }

    #[test]
    fn lives_in_paris_emits_one_triple() {
        let text = "Sarah lives in Paris";
        let chunks = chunks_for(text);
        let rels = extract_svo_triples(text, &chunks);
        assert_eq!(rels.len(), 1, "expected 1 triple, got {rels:?}");
        let r = &rels[0];
        assert_eq!(&text[r.subject_char_start..r.subject_char_end], "Sarah");
        assert_eq!(r.attribute_name, "lives in");
        let (s, e) = obj_span(r);
        assert_eq!(&text[s..e], "Paris");
    }

    #[test]
    fn works_at_google_emits_one_triple() {
        let text = "John works at Google";
        let chunks = chunks_for(text);
        let rels = extract_svo_triples(text, &chunks);
        assert_eq!(rels.len(), 1, "expected 1 triple, got {rels:?}");
        assert_eq!(rels[0].attribute_name, "works at");
    }

    #[test]
    fn coordination_keeps_same_subject() {
        let text = "Sarah works at Google and lives in Paris";
        let chunks = chunks_for(text);
        let rels = extract_svo_triples(text, &chunks);
        assert_eq!(rels.len(), 2, "expected 2 triples, got {rels:?}");
        for r in &rels {
            assert_eq!(
                &text[r.subject_char_start..r.subject_char_end],
                "Sarah",
                "all triples should share Sarah as subject"
            );
        }
        assert_eq!(rels[0].attribute_name, "works at");
        assert_eq!(rels[1].attribute_name, "and lives in");
        let (s0, e0) = obj_span(&rels[0]);
        let (s1, e1) = obj_span(&rels[1]);
        assert_eq!(&text[s0..e0], "Google");
        assert_eq!(&text[s1..e1], "Paris");
    }

    #[test]
    fn empty_input_emits_nothing() {
        let rels = extract_svo_triples("", &[]);
        assert!(rels.is_empty());
    }

    #[test]
    fn single_word_emits_nothing() {
        let text = "Hello";
        let chunks = chunks_for(text);
        let rels = extract_svo_triples(text, &chunks);
        assert!(rels.is_empty());
    }

    #[test]
    fn copular_sentence_misses_quietly() {
        let text = "Sarah is happy";
        let chunks = chunks_for(text);
        let rels = extract_svo_triples(text, &chunks);
        assert!(rels.is_empty(), "expected no triples; got {rels:?}");
    }

    #[test]
    fn punctuation_bounds_the_phrase() {
        let text = "Sarah lives in Paris. John works at Google.";
        let chunks = chunks_for(text);
        let rels = extract_svo_triples(text, &chunks);
        assert_eq!(rels.len(), 2, "expected 2 triples, got {rels:?}");
        assert_eq!(
            &text[rels[0].subject_char_start..rels[0].subject_char_end],
            "Sarah"
        );
        assert_eq!(
            &text[rels[1].subject_char_start..rels[1].subject_char_end],
            "John"
        );
    }

    #[test]
    fn is_verb_shape_handles_common_cases() {
        assert!(is_verb_shape("lives"));
        assert!(is_verb_shape("works"));
        assert!(is_verb_shape("opened"));
        assert!(is_verb_shape("running"));
        assert!(is_verb_shape("taken"));
        assert!(!is_verb_shape("Sarah"));
        assert!(!is_verb_shape("Paris"));
        assert!(!is_verb_shape("happy"));
        assert!(is_verb_shape("kids"));
        assert!(is_verb_shape("series"));
        assert!(!is_verb_shape("Paris"));
        assert!(!is_verb_shape("James"));
        assert!(!is_verb_shape("glass"));
        assert!(!is_verb_shape("focus"));
        assert!(is_verb_shape("is"));
        assert!(!is_verb_shape("in"));
    }
}
