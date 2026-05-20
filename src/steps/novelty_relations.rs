//! Novelty-branch relation extraction (pattern OpenIE).
//!
//! When the known-branch extractors (NER + pattern RE templates over
//! seed-pack attributes) don't fire, the novelty branch still needs to
//! capture *that two content tokens were connected by something*. This
//! module does that, purely from surface text — no NER, no model, no
//! POS tagger, no verb lexicon. The void filter has already dropped
//! function words; what remains plus a verb-shape morphology check is
//! enough to produce candidate `(subject, attribute_text, object)`
//! triples.
//!
//! Every output is a candidate, not an assertion. Step 8 will resolve
//! `attribute_text` against existing attribute-name Elements (or mint
//! a Defeasible one) and stamp the relation `Defeasible`. Replay
//! confirms or prunes per the normal `policy.promotion_*` gates.
//!
//! ## Algorithm
//!
//! For each `Phrase` chunk in the novelty stream, collect the `Token`
//! chunks that fall inside it (the void filter has already dropped
//! function words). Find tokens whose surface form looks like a verb:
//! ends in `-s` (excluding `-ss`/`-us`), `-ed`, `-ing`, or `-en`,
//! length ≥ 3.
//!
//! Subject is sticky across verbs in the same phrase: the run of
//! content tokens before the *first* verb-shape token in the phrase.
//! This preserves coordination — "Sarah works at X and lives in Y"
//! attaches both predicates to Sarah, not to the intermediate object.
//!
//! Object for verb V_k = content tokens after V_k up to (but not
//! including) the next verb-shape token in the phrase.
//!
//! Attribute text = the raw input between subject's end and object's
//! start, trimmed. Captures the verb + connectives ("lives in",
//! "works at", "changed the").
//!
//! ## What this catches
//!
//! - "Sarah lives in Paris" → (Sarah, "lives in", Paris)
//! - "John works at Google" → (John, "works at", Google)
//! - "Dr Rao changed the appointment" → (Dr Rao, "changed the", appointment)
//! - "Sarah works at X and lives in Y" → two triples, both subject=Sarah
//!
//! ## What this misses (acceptable; defer to replay or known branch)
//!
//! - Copular sentences ("Sarah is happy"): `is` is void; remaining
//!   tokens are Sarah + happy with no verb-shape between them.
//! - Irregular past tense ("saw", "ran", "gave", "took"): no
//!   qualifying suffix.
//! - Sentences without a Phrase boundary: bound by the Phrase pass.

use crate::steps::orthographic::{ChunkScale, OrthographicChunk};

/// A candidate `(subject, attribute_text, object)` triple from the
/// novelty branch's pattern OpenIE pass. Subject and object are
/// character spans into the original input; `attribute_text` is the
/// verbatim connective (verb + surrounding void words), to be
/// resolved against attribute-name Elements by Step 8.
#[derive(Debug, Clone)]
pub struct NoveltyRelation {
    pub subject_char_start: usize,
    pub subject_char_end: usize,
    /// Trimmed text between the subject and object spans. Will be
    /// resolved to an attribute-name Element by Step 8 (lookup →
    /// embedding search → mint Defeasible if no match).
    pub attribute_text: String,
    pub object_char_start: usize,
    pub object_char_end: usize,
    /// Heuristic confidence. Always low — these are candidates for
    /// replay confirmation, not assertions. Step 8 will stamp the
    /// resulting Relation `Defeasible`.
    pub confidence: f32,
}

/// Default confidence stamped on every novelty relation. Low enough
/// that any known-branch proposal on the same span will outrank it
/// in Step 8's merge.
const DEFAULT_CONFIDENCE: f32 = 0.4;

/// Extract candidate relations from the novelty stream produced by
/// Step 5a's orthographic chunker + void filter. The input slice
/// must contain both `Phrase`-scale and `Token`-scale chunks; this
/// function filters internally.
pub fn extract_novelty_relations(
    text: &str,
    novelty_chunks: &[OrthographicChunk],
) -> Vec<NoveltyRelation> {
    let mut out = Vec::new();

    for phrase in novelty_chunks
        .iter()
        .filter(|c| c.scale == ChunkScale::Phrase)
    {
        let tokens: Vec<&OrthographicChunk> = novelty_chunks
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
            .filter(|(_, t)| is_verb_shape(&t.text))
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
                // No content subject either — skip.
                continue;
            } else {
                (
                    tokens[0].char_start,
                    tokens[subj_end_idx - 1].char_end,
                    None,
                )
            };

        for (k, &vi) in verb_idxs.iter().enumerate() {
            // Object = content tokens after this verb, up to but not
            // including the next verb-shape token (or end of phrase).
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
            // and the *previous object's* tail for k>0. Using
            // subj_last for k>0 would swallow the previous object
            // into the attribute text. When the subject is a
            // synthesized pronoun, anchor the attribute span at the
            // pronoun's end so adverbial modifiers between the
            // pronoun and verb ("recently" in "I recently got X")
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

            out.push(NoveltyRelation {
                subject_char_start: subj_first_start,
                subject_char_end: subj_last_end,
                attribute_text: attr_text,
                object_char_start: obj_first.char_start,
                object_char_end: obj_last.char_end,
                confidence: DEFAULT_CONFIDENCE,
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
/// the original text. Returns `None` if the preamble carries no
/// pronoun. Case-insensitive — `"I went"` and `"i went"` both work.
fn find_pronoun_subject(text: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    let slice = &text[start..end];
    // Tokenize on whitespace; check each token against the pronoun
    // set. Stops at the first match. Strips edge punctuation so
    // `"I,"` still matches `"i"`.
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
        cursor += raw.len() + 1; // +1 for the whitespace we split on
    }
    None
}

/// High-frequency irregular verbs that the suffix-only detector
/// misses. Past, present, and base forms of the 100-ish most common
/// English verbs that don't end in `-ed`/`-ing`/`-en`/`-s`. Hand-
/// curated from Zipf-frequency lists; favors verbs that change a
/// substrate's view of the world (acquire/transfer/move/state)
/// over modal auxiliaries (might, could) which don't head their
/// own clauses well in our (sticky-subject, verb-segmented) parse.
///
/// Sorted; case-insensitive membership test runs in O(log N).
/// Length kept under 200 entries so the binary-search probe is
/// cheap on every Token chunk.
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

/// True if `token` is in the irregular-verb lexicon (case-insensitive).
/// `is_verb_shape` calls this first so common base/past-tense
/// irregulars get caught alongside the suffix-rule cases.
fn is_irregular_verb(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    IRREGULAR_VERBS.binary_search(&lower.as_str()).is_ok()
}

/// True if `token`'s surface form has a verb-shape morphology cue:
/// ends in `-ed`, `-ing`, or `-en`, or — for lowercase tokens only —
/// ends in `-s` (excluding plural-noun shapes `-ss`, `-us`).
/// Also catches high-frequency irregular verbs via the lexicon.
///
/// The lowercase guard on the `-s` rule rejects proper nouns like
/// `Paris` / `James` / `Sales` that would otherwise dominate as
/// fake verbs. Capitalized verbs ("Opened", "Running") are still
/// caught via the unconditional `-ed`/`-ing`/`-en` rules.
///
/// Imperfect on purpose. False positives ("kids", "series") and
/// remaining false negatives (rare irregulars not in the lexicon)
/// are accepted; replay confirms or prunes.
fn is_verb_shape(token: &str) -> bool {
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

    #[test]
    fn irregular_verb_lexicon_is_sorted_and_dedup() {
        // `is_irregular_verb` uses binary_search which assumes the
        // slice is sorted. Catch any out-of-order edit at test time.
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
        // Past-tense irregulars that the suffix-only rule missed:
        // got, ran, saw, ate, took, made, gave, came, went, said,
        // had, was, were, bought, brought, taught, sent, kept.
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
        // Sentence-initial "Got" / "Ran" / "Bought" should still
        // hit via the lexicon (case-insensitive).
        for v in ["Got", "Ran", "Bought", "WAS"] {
            assert!(is_verb_shape(v), "`{v}` (capitalized) should be detected");
        }
    }

    #[test]
    fn non_verbs_still_not_detected() {
        // Sanity: random nouns / adjectives we did NOT add to the
        // lexicon stay out. (Plurals like `cars` still hit the -s
        // rule, and `red` hits the -ed rule — both are pre-existing
        // imperfect behavior, accepted.)
        for w in ["the", "very", "big", "house", "phone"] {
            assert!(!is_verb_shape(w), "`{w}` shouldn't be a verb");
        }
    }

    #[test]
    fn got_phrase_forms_a_relation() {
        // The Q4 case: "I got a Samsung Galaxy S22 on February 20th"
        // previously yielded no novelty triple because "got" isn't
        // suffix-shaped. With the lexicon, it should.
        let text = "I got a Samsung Galaxy S22 from Best Buy on February 20th";
        let chunks = chunks_for(text);
        let rels = extract_novelty_relations(text, &chunks);
        assert!(
            !rels.is_empty(),
            "expected ≥1 triple from `I got a Samsung Galaxy S22 …`, got 0"
        );
        // Subject should be the leading "I"; first verb is `got`.
        let r = &rels[0];
        assert_eq!(&text[r.subject_char_start..r.subject_char_end], "I");
        assert!(
            r.attribute_text.contains("got"),
            "attribute should include `got`; got {:?}",
            r.attribute_text
        );
    }

    #[test]
    fn lives_in_paris_emits_one_triple() {
        let text = "Sarah lives in Paris";
        let chunks = chunks_for(text);
        let rels = extract_novelty_relations(text, &chunks);
        assert_eq!(rels.len(), 1, "expected 1 triple, got {rels:?}");
        let r = &rels[0];
        assert_eq!(&text[r.subject_char_start..r.subject_char_end], "Sarah");
        assert_eq!(r.attribute_text, "lives in");
        assert_eq!(&text[r.object_char_start..r.object_char_end], "Paris");
    }

    #[test]
    fn works_at_google_emits_one_triple() {
        let text = "John works at Google";
        let chunks = chunks_for(text);
        let rels = extract_novelty_relations(text, &chunks);
        assert_eq!(rels.len(), 1, "expected 1 triple, got {rels:?}");
        assert_eq!(rels[0].attribute_text, "works at");
    }

    #[test]
    fn coordination_keeps_same_subject() {
        // "Sarah works at Google and lives in Paris" — both verbs
        // should attach to Sarah, not have the second's subject be
        // Google.
        let text = "Sarah works at Google and lives in Paris";
        let chunks = chunks_for(text);
        let rels = extract_novelty_relations(text, &chunks);
        assert_eq!(rels.len(), 2, "expected 2 triples, got {rels:?}");
        for r in &rels {
            assert_eq!(
                &text[r.subject_char_start..r.subject_char_end],
                "Sarah",
                "all triples should share Sarah as subject"
            );
        }
        assert_eq!(rels[0].attribute_text, "works at");
        // Second verb's attr starts after the previous object
        // (Google), not after the original subject. So it should
        // contain the conjunction + the second verb-phrase, but NOT
        // contain the first object's text.
        assert_eq!(rels[1].attribute_text, "and lives in");
        // Each object stays distinct: triple 1's object is Google,
        // triple 2's object is Paris.
        assert_eq!(
            &text[rels[0].object_char_start..rels[0].object_char_end],
            "Google"
        );
        assert_eq!(
            &text[rels[1].object_char_start..rels[1].object_char_end],
            "Paris"
        );
    }

    #[test]
    fn empty_input_emits_nothing() {
        let rels = extract_novelty_relations("", &[]);
        assert!(rels.is_empty());
    }

    #[test]
    fn single_word_emits_nothing() {
        let text = "Hello";
        let chunks = chunks_for(text);
        let rels = extract_novelty_relations(text, &chunks);
        assert!(rels.is_empty());
    }

    #[test]
    fn copular_sentence_misses_quietly() {
        // "Sarah is happy" — "is" is a void auxiliary, so after the
        // filter the only content tokens are Sarah + happy. Neither
        // has verb-shape morphology; we emit nothing rather than a
        // wrong triple.
        let text = "Sarah is happy";
        let chunks = chunks_for(text);
        let rels = extract_novelty_relations(text, &chunks);
        assert!(rels.is_empty(), "expected no triples; got {rels:?}");
    }

    #[test]
    fn punctuation_bounds_the_phrase() {
        // Two separate phrases. The "lives in Paris" branch should
        // fire; "works at Google" should fire independently. Neither
        // crosses the period.
        let text = "Sarah lives in Paris. John works at Google.";
        let chunks = chunks_for(text);
        let rels = extract_novelty_relations(text, &chunks);
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
        // True positives
        assert!(is_verb_shape("lives"));
        assert!(is_verb_shape("works"));
        assert!(is_verb_shape("opened"));
        assert!(is_verb_shape("running"));
        assert!(is_verb_shape("taken"));

        // True negatives
        assert!(!is_verb_shape("Sarah"));
        assert!(!is_verb_shape("Paris"));
        assert!(!is_verb_shape("happy"));

        // Known false positives — accepted, marked Defeasible at use
        assert!(is_verb_shape("kids")); // plural noun
        assert!(is_verb_shape("series")); // singular noun, looks plural

        // Proper nouns ending in -s — capitalization guard rejects
        assert!(!is_verb_shape("Paris"));
        assert!(!is_verb_shape("James"));

        // Excluded false-positive shapes
        assert!(!is_verb_shape("glass")); // -ss
        assert!(!is_verb_shape("focus")); // -us

        // `is` is in the irregular-verb lexicon (it's a copula),
        // so it now counts as a verb-shape token. Note: it'll
        // never actually reach this check via the novelty pass
        // because `is` is seeded as a Void element and gets
        // filtered out by `extract_content_tokens`.
        assert!(is_verb_shape("is"));
        // `in` is a preposition, not a verb, and not in the
        // lexicon — stays excluded.
        assert!(!is_verb_shape("in"));
    }
}
