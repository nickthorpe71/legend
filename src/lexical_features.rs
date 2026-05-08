//! Hand-crafted lexical features for intent detection.
//!
//! These features sit alongside the BGE/MiniLM sentence embedding when
//! training and querying the per-dimension intent classifiers. They serve
//! as a *front-door mediator* (Pearl): linguistic features are causally
//! upstream of the embedding (the speaker's intent → word choice →
//! embedding), so they capture intent signal that the embedding represents
//! confounded with topic.
//!
//! Output is a fixed-size `[f32; LEXICAL_FEATURE_COUNT]` vector per input.
//! Counts are mostly density-normalized (count / token_count) so longer
//! inputs aren't dominated by raw count features.

pub const LEXICAL_FEATURE_COUNT: usize = 34;

pub fn extract_lexical_features(text: &str) -> [f32; LEXICAL_FEATURE_COUNT] {
    let lower = text.to_lowercase();
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    let token_count = tokens.len().max(1) as f32;
    let len_inv = 1.0 / token_count;

    let first_token = tokens.first().copied().unwrap_or("");
    let first_token_clean = first_token.trim_matches(|c: char| !c.is_alphabetic());

    let mut f = [0.0f32; LEXICAL_FEATURE_COUNT];

    // ── Pronouns (0..3) ──────────────────────────────────────────────
    f[0] = count_token_in(&tokens, FIRST_PERSON_SINGULAR) as f32 * len_inv;
    f[1] = count_token_in(&tokens, FIRST_PERSON_PLURAL) as f32 * len_inv;
    f[2] = count_token_in(&tokens, SECOND_PERSON) as f32 * len_inv;
    f[3] = count_token_in(&tokens, THIRD_PERSON) as f32 * len_inv;

    // ── Modals & certainty (4..9) ────────────────────────────────────
    f[4] = count_token_in(&tokens, STRONG_EPISTEMIC) as f32 * len_inv;
    f[5] = count_token_in(&tokens, WEAK_EPISTEMIC) as f32 * len_inv;
    f[6] = count_token_in(&tokens, DEONTIC) as f32 * len_inv;
    f[7] = count_substring(&lower, CERTAINTY_IDIOMS) as f32 * len_inv;
    f[8] = count_substring(&lower, UNCERTAINTY_IDIOMS) as f32 * len_inv;
    f[9] = count_token_in(&tokens, INTENSIFIER_QUANT) as f32 * len_inv;

    // ── Question shape (10..13) ──────────────────────────────────────
    f[10] = if text.contains('?') { 1.0 } else { 0.0 };
    f[11] = count_token_in(&tokens, WH_WORDS) as f32 * len_inv;
    f[12] = if INTERROGATIVE_AUX_START.contains(&first_token_clean) {
        1.0
    } else {
        0.0
    };
    f[13] = if IMPERATIVE_QUERY_START.contains(&first_token_clean) {
        1.0
    } else {
        0.0
    };

    // ── Negation (14..15) ────────────────────────────────────────────
    f[14] = (count_token_in(&tokens, DIRECT_NEGATION) + count_substring(&lower, &["n't"])) as f32
        * len_inv;
    f[15] = count_token_in(&tokens, NEGATION_IDIOMS) as f32 * len_inv;

    // ── Correction / revision / discovery (16..18) ───────────────────
    f[16] = count_substring(&lower, CORRECTION_MARKERS) as f32 * len_inv;
    f[17] = count_substring(&lower, REVISION_MARKERS) as f32 * len_inv;
    f[18] = count_substring(&lower, DISCOVERY_MARKERS) as f32 * len_inv;

    // ── Continuity (19..20) ──────────────────────────────────────────
    f[19] = count_substring(&lower, CONTINUITY_MARKERS) as f32 * len_inv;
    f[20] = count_token_in(&tokens, REPETITION_TOKENS) as f32 * len_inv;

    // ── Emotional intensity (21..24) ─────────────────────────────────
    f[21] = count_token_in(&tokens, INTENSITY_ADVERBS) as f32 * len_inv;
    f[22] = count_token_in(&tokens, EMERGENCY_TOKENS) as f32 * len_inv;
    f[23] = count_token_in(&tokens, POSITIVE_EMOTION) as f32 * len_inv;
    f[24] = count_token_in(&tokens, NEGATIVE_EMOTION) as f32 * len_inv;

    // ── Importance / dismissal (25..26) ──────────────────────────────
    f[25] = count_token_in(&tokens, IMPORTANCE_TOKENS) as f32 * len_inv;
    f[26] = count_substring(&lower, DISMISSAL_MARKERS) as f32 * len_inv;

    // ── Punctuation (27..30) ─────────────────────────────────────────
    f[27] = text.chars().filter(|&c| c == '!').count() as f32 * len_inv;
    f[28] = text.chars().filter(|&c| c == '?').count() as f32 * len_inv;
    f[29] = (text.matches('—').count() + text.matches("--").count()) as f32 * len_inv;
    f[30] = caps_ratio(&tokens);

    // ── Tense markers (31..32) ───────────────────────────────────────
    f[31] = count_token_in(&tokens, PAST_TENSE_MARKERS) as f32 * len_inv;
    f[32] = count_token_in(&tokens, FUTURE_TENSE_MARKERS) as f32 * len_inv;

    // ── Length (33) ──────────────────────────────────────────────────
    // log(token_count + 1) gives a smooth length signal that doesn't blow
    // up on long inputs. Roughly: 1-token → 0.69, 5 → 1.79, 30 → 3.43.
    f[33] = (token_count + 1.0).ln();

    f
}

// ── Word lists ──────────────────────────────────────────────────────────

const FIRST_PERSON_SINGULAR: &[&str] = &[
    "i", "me", "my", "mine", "myself", "i'm", "i've", "i'd", "i'll",
];
const FIRST_PERSON_PLURAL: &[&str] = &["we", "us", "our", "ours", "ourselves", "we've", "we'll"];
const SECOND_PERSON: &[&str] = &["you", "your", "yours", "yourself", "yourselves", "you're"];
const THIRD_PERSON: &[&str] = &[
    "he",
    "she",
    "it",
    "they",
    "them",
    "his",
    "her",
    "their",
    "theirs",
    "itself",
    "themselves",
];

const STRONG_EPISTEMIC: &[&str] = &[
    "will",
    "certainly",
    "surely",
    "undoubtedly",
    "definitely",
    "obviously",
    "clearly",
    "indisputably",
];
const WEAK_EPISTEMIC: &[&str] = &[
    "might",
    "may",
    "could",
    "would",
    "possibly",
    "perhaps",
    "maybe",
    "probably",
    "presumably",
    "supposedly",
];
const DEONTIC: &[&str] = &["should", "ought", "must", "shall"];

const CERTAINTY_IDIOMS: &[&str] = &[
    "without a doubt",
    "no question",
    "no doubt",
    "for sure",
    "make no mistake",
    "you can count on",
];
const UNCERTAINTY_IDIOMS: &[&str] = &[
    "i think",
    "not sure",
    "i guess",
    "i suppose",
    "best guess",
    "have a hunch",
    "leaning toward",
    "tentatively",
    "could be wrong",
];
const INTENSIFIER_QUANT: &[&str] = &[
    "completely",
    "fully",
    "totally",
    "entirely",
    "wholly",
    "100%",
];

const WH_WORDS: &[&str] = &[
    "what", "where", "when", "who", "whom", "why", "how", "which", "whose",
];
const INTERROGATIVE_AUX_START: &[&str] = &[
    "do", "does", "did", "have", "has", "had", "is", "are", "was", "were", "can", "could",
    "should", "would", "will", "may", "might",
];
const IMPERATIVE_QUERY_START: &[&str] = &[
    "find", "look", "check", "show", "tell", "remind", "search", "get", "fetch", "retrieve",
    "recall", "lookup",
];

const DIRECT_NEGATION: &[&str] = &["not", "no"];
const NEGATION_IDIOMS: &[&str] = &["never", "nothing", "none", "neither", "nobody", "nowhere"];

const CORRECTION_MARKERS: &[&str] = &[
    "actually",
    "wait",
    "scratch",
    "correction",
    "no wait",
    "hold on",
    "oops",
    "let me correct",
    "strike that",
];
const REVISION_MARKERS: &[&str] = &[
    "take that back",
    "revise",
    "second thought",
    "ignore what",
    "retract",
    "i was wrong",
    "i was mistaken",
    "had it backwards",
];
const DISCOVERY_MARKERS: &[&str] = &[
    "just learned",
    "turns out",
    "i realized",
    "never realized",
    "surprisingly",
    "i hadn't realized",
];

const CONTINUITY_MARKERS: &[&str] = &[
    "as usual",
    "as expected",
    "like before",
    "same as",
    "nothing new",
    "no surprises",
    "as predicted",
    "right on track",
    "business as usual",
];
const REPETITION_TOKENS: &[&str] = &["again", "still", "always", "usually", "consistently"];

const INTENSITY_ADVERBS: &[&str] = &[
    "incredibly",
    "extremely",
    "deeply",
    "absolutely",
    "very",
    "really",
    "terribly",
    "exceptionally",
];
const EMERGENCY_TOKENS: &[&str] = &[
    "urgent",
    "emergency",
    "critical",
    "disaster",
    "crisis",
    "asap",
    "immediately",
    "now",
];
const POSITIVE_EMOTION: &[&str] = &[
    "love",
    "excited",
    "thrilled",
    "happy",
    "wonderful",
    "amazing",
    "fantastic",
    "joy",
    "great",
];
const NEGATIVE_EMOTION: &[&str] = &[
    "hate",
    "furious",
    "devastated",
    "upset",
    "horrified",
    "angry",
    "awful",
    "terrible",
    "worried",
];

const IMPORTANCE_TOKENS: &[&str] = &[
    "important",
    "matters",
    "crucial",
    "vital",
    "essential",
    "significant",
];
const DISMISSAL_MARKERS: &[&str] = &[
    "fyi",
    "btw",
    "by the way",
    "side note",
    "incidentally",
    "minor",
    "trivial",
    "just noting",
    "no rush",
    "no hurry",
    "low priority",
    "not a big deal",
];

const PAST_TENSE_MARKERS: &[&str] = &[
    "yesterday",
    "ago",
    "last",
    "previously",
    "earlier",
    "before",
    "was",
    "were",
];
const FUTURE_TENSE_MARKERS: &[&str] = &["tomorrow", "next", "soon", "later", "shortly", "upcoming"];

// ── Helpers ─────────────────────────────────────────────────────────────

fn count_token_in(tokens: &[&str], words: &[&str]) -> usize {
    tokens
        .iter()
        .filter(|t| {
            let cleaned = t.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '%');
            words.contains(&cleaned)
        })
        .count()
}

fn count_substring(text: &str, phrases: &[&str]) -> usize {
    phrases.iter().map(|p| text.matches(p).count()).sum()
}

fn caps_ratio(tokens: &[&str]) -> f32 {
    let total = tokens.len().max(1) as f32;
    let caps = tokens
        .iter()
        .filter(|t| {
            t.len() > 1
                && t.chars().all(|c| !c.is_alphabetic() || c.is_uppercase())
                && t.chars().any(|c| c.is_alphabetic())
        })
        .count() as f32;
    caps / total
}
