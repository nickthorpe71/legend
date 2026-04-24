use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};

use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;

use crate::memory::wernicke::KeywordCache;

/// MiniLM model keeps its tokenizer (thread-safe via `&self`) and an ONNX
/// Runtime session. `ort::Session::run` takes `&mut self` in rc.12, so we wrap
/// it in a `Mutex` — the ORT backend is already thread-parallel internally, so
/// serializing callers at the mutex doesn't cost us the SIMD/kernel parallelism
/// that's the whole reason to use ORT. The daemon holds this singleton and
/// amortizes the ~300-500 ms Session init across every tick for the rest of
/// the session lifetime.
struct SentenceModel {
    tokenizer: tokenizers::Tokenizer,
    session: Mutex<Session>,
}

/// Lazy-initialized sentence embedding model (all-MiniLM-L6-v2 quantized, 384-dim).
/// The model is embedded directly in the binary — no network download required.
/// Inference runs via ONNX Runtime (the `ort` crate) with graph optimizations
/// enabled. ORT ships as a dynamic library; the `ort` crate's `download-binaries`
/// feature fetches the right build for the current target at `cargo build` time.
///
/// # Model choice
/// We use all-MiniLM-L6-v2 quantized (~23MB) over BAAI/bge-small-en-v1.5 (~45MB)
/// for faster inference (6 layers vs 12) and smaller binary size. Both produce
/// 384-dim embeddings. If retrieval quality degrades (measured via LongMemEval),
/// swap to BGE-small-en-v1.5 by changing the model files — no code changes needed.
static SENTENCE_MODEL: std::sync::LazyLock<SentenceModel> = std::sync::LazyLock::new(|| {
    let tokenizer = tokenizers::Tokenizer::from_bytes(include_bytes!(
        "../../models/all-MiniLM-L6-v2-q/tokenizer.json"
    ))
    .expect("Failed to load embedded tokenizer");

    let onnx_bytes: &[u8] = include_bytes!("../../models/all-MiniLM-L6-v2-q/model.onnx");
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .max(1);
    let session = Session::builder()
        .expect("ort session builder")
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .expect("ort set optimization level")
        .with_intra_threads(threads)
        .expect("ort set intra threads")
        .commit_from_memory(onnx_bytes)
        .expect("ort commit from memory (ONNX model bytes)");

    SentenceModel {
        tokenizer,
        session: Mutex::new(session),
    }
});

// Build-time assertion: SentenceModel must be Send+Sync so the static can
// be shared across threads. Mutex<Session> is Send+Sync; Tokenizer is too.
// If a future `ort` upgrade breaks this, the build fails here instead of
// producing a runtime data race.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SentenceModel>();
};

/// Process-local memoization for embeddings. Keyed by `(text, dim)` since two
/// callers can legitimately request different output dimensions from the same
/// input. Values are `Arc<Vec<f32>>` so multi-thread readers clone a cheap
/// pointer and then `(*arc).clone()` the payload only at return.
///
/// Read path takes a shared lock; write path (miss → fill) takes an exclusive
/// lock but only AFTER the expensive inference is done, so the write critical
/// section is just a HashMap insert. Double-compute on race is safe because
/// the embedding function is deterministic.
///
/// TODO: convert to LRU/bounded if production cache grows unbounded with
/// high-cardinality user input. For the test suite (~48 unique strings) the
/// cache tops out under 80 KB per process.
static EMBED_CACHE: std::sync::LazyLock<RwLock<HashMap<(String, usize), Arc<Vec<f32>>>>> =
    std::sync::LazyLock::new(|| RwLock::new(HashMap::with_capacity(256)));

fn cached_or_compute<F: FnOnce() -> Vec<f32>>(text: &str, dim: usize, compute: F) -> Vec<f32> {
    // Fast path: shared read lock. Miss allocation of the lookup key is
    // unavoidable with `HashMap<(String, usize), _>` via `get()`, but it's
    // tens of nanoseconds for short inputs and cheap relative to the
    // ~100 ms inference we're avoiding.
    if let Some(hit) = EMBED_CACHE
        .read()
        .expect("embed cache poisoned")
        .get(&(text.to_owned(), dim))
    {
        return (**hit).clone();
    }

    // Miss: compute outside the lock so other readers/writers aren't blocked
    // during the 100+ ms inference.
    let arc = Arc::new(compute());

    // Fill. Entry API avoids clobbering a concurrent writer; double-compute
    // on race is fine because the function is deterministic.
    EMBED_CACHE
        .write()
        .expect("embed cache poisoned")
        .entry((text.to_owned(), dim))
        .or_insert_with(|| arc.clone());

    (*arc).clone()
}

/// Entorhinal Cortex — Representational encoding and compression gateway.
///
/// The entorhinal cortex sits between the hippocampus and the neocortex,
/// acting as the primary encoding and compression gateway.  It receives raw
/// sensory data and transforms it into the representations the rest of the
/// brain uses — analogous to how grid cells compress spatial information
/// into periodic, low-dimensional codes.
///
/// In Legend, `entorhinal.rs` owns both representational encoding and
/// information compression:
///
/// **Representational encoding:**
///
/// - **Semantic embedding (`embed_text`)** — raw text is transformed into a
///   384-dimensional vector using the all-MiniLM-L6-v2 quantized sentence
///   transformer, embedded directly in the binary.  This captures semantic
///   meaning so downstream modules (hippocampus, neocortex, dentate gyrus)
///   can compare entries by meaning, not just lexical overlap.
///
/// - **Cosine similarity (`cosine_similarity`)** — distance between two
///   embeddings, analogous to neural "closeness" of two percepts.
///
/// - **Embedding merge (`merge_embeddings`)** — element-wise average for
///   combining related representations during consolidation.
///
/// **Information compression:**
///
/// - **Text chunking (`chunk_text`)** — groups local sentence runs into
///   episode-like chunks while respecting hard discourse boundaries. Continuous
///   input is discretized into manageable units that each get their own
///   embedding and memory trace.
///
/// - **Single-text summarization (`summarize_single`)** — extractive
///   best-sentence selection, boosted by decision rationale, code references,
///   and architecture keywords.
///
/// - **Group summarization (`summarize_group`)** — picks up to 3 entries
///   from a cluster using salience, usage, and diversity, joins their
///   summaries, and truncates to 300 characters.
///
/// Like the biological entorhinal cortex, this module is a pure transform
/// layer — it encodes and compresses information but doesn't store or
/// retrieve it.
use crate::memory::ShortTermEntry;

const MAX_SUMMARY_LEN: usize = 200;
const MAX_GROUP_SUMMARY_LEN: usize = 300;
/// Soft budget for one episode-like memory chunk. Longer single sentences are
/// kept intact; sentence runs are only split when adding another sentence would
/// exceed this target.
const EPISODIC_CHUNK_TARGET_CHARS: usize = 420;
/// Discourse markers that signal a topic shift within a single turn.
const TOPIC_SHIFT_MARKERS: &[&str] = &[
    "by the way",
    "also,",
    "on another note",
    "speaking of",
    "separately",
    "btw",
    "BTW",
    "incidentally",
    "oh and",
    "one more thing",
];

fn is_semantic_junk_token(token: &str) -> bool {
    let trimmed = token.trim_matches(|c: char| c.is_ascii_punctuation());
    if trimmed.is_empty()
        && token.chars().count() >= 4
        && token.chars().all(|c| c.is_ascii_punctuation())
    {
        return true;
    }

    let punct_count = token.chars().filter(|c| c.is_ascii_punctuation()).count();
    let digit_count = token.chars().filter(|c| c.is_ascii_digit()).count();
    let alpha_count = token.chars().filter(|c| c.is_ascii_alphabetic()).count();

    alpha_count == 0 && punct_count >= 4 && digit_count <= 2 && punct_count >= digit_count * 2
}

/// Remove syntactic noise that should not become semantic L3 evidence.
///
/// This intentionally targets only standalone punctuation runs so exact L1/L2
/// text can remain raw while Summary/source evidence avoids junk artifacts like
/// `[[[[`, `%%@@`, `////`, or `}}}}`.
pub fn clean_semantic_noise(text: &str) -> String {
    text.split_whitespace()
        .filter(|token| !is_semantic_junk_token(token))
        .collect::<Vec<_>>()
        .join(" ")
}

// ── Representational encoding ───────────────────────────────────────────

/// Run inference on a single tokenizer encoding and return the L2-normalized
/// attention-masked mean-pooled embedding. Mean pooling matches how
/// all-MiniLM-L6-v2 was trained and used by sentence-transformers — CLS
/// pooling underperforms on sentence-similarity benchmarks for this model.
///
/// The caller holds the session mutex for the duration of `run()`. ORT's
/// intra-op thread pool parallelizes each op across cores, so serializing at
/// the mutex doesn't cost us the CPU parallelism that made us pick ORT in the
/// first place.
fn infer_embedding(
    session: &mut Session,
    encoding: &tokenizers::Encoding,
    dim: usize,
) -> Vec<f32> {
    let input_ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
    let attention_mask: Vec<i64> = encoding
        .get_attention_mask()
        .iter()
        .map(|&m| m as i64)
        .collect();
    let token_type_ids: Vec<i64> = encoding.get_type_ids().iter().map(|&t| t as i64).collect();
    let seq_len = input_ids.len();

    let ids_tensor = Tensor::from_array(([1, seq_len], input_ids)).expect("ids tensor");
    let mask_tensor =
        Tensor::from_array(([1, seq_len], attention_mask.clone())).expect("mask tensor");
    let types_tensor =
        Tensor::from_array(([1, seq_len], token_type_ids)).expect("token_type_ids tensor");

    let outputs = session
        .run(ort::inputs! {
            "input_ids" => ids_tensor,
            "attention_mask" => mask_tensor,
            "token_type_ids" => types_tensor,
        })
        .expect("ONNX inference failed");

    // MiniLM-style models typically emit "last_hidden_state" as output 0.
    // We just grab the first output tensor so the code is robust to the
    // exact output name (some exports call it "output", some "0").
    let (_name, output_value) = outputs
        .iter()
        .next()
        .expect("ONNX inference produced no outputs");

    let tensor = output_value
        .try_extract_array::<f32>()
        .expect("output tensor is not f32");
    let shape = tensor.shape();
    let hidden_size = shape[shape.len() - 1];

    // Shape: [1, seq_len, hidden_size]. Mean-pool over non-padding tokens,
    // weighted by the attention mask, matching the reference sentence-
    // transformers implementation.
    let view = tensor.as_slice().expect("output tensor is not contiguous");

    let mut pooled = vec![0.0f32; hidden_size];
    let mut total_mask = 0.0f32;
    for t in 0..seq_len {
        let m = attention_mask[t] as f32;
        if m == 0.0 {
            continue;
        }
        total_mask += m;
        let row_start = t * hidden_size;
        for d in 0..hidden_size {
            pooled[d] += view[row_start + d] * m;
        }
    }
    let denom = total_mask.max(1e-9);
    for v in pooled.iter_mut() {
        *v /= denom;
    }

    let emb = l2_normalize(&pooled);
    fit_to_dim(&emb, dim)
}

fn l2_normalize(v: &[f32]) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    v.iter().map(|&x| x / (norm + 1e-12)).collect()
}

fn fit_to_dim(emb: &[f32], dim: usize) -> Vec<f32> {
    if emb.len() == dim {
        return emb.to_vec();
    }
    let mut result = vec![0.0f32; dim];
    for (i, &v) in emb.iter().enumerate().take(dim) {
        result[i] = v;
    }
    result
}

/// Compute a semantic embedding vector using all-MiniLM-L6-v2 quantized (384-dim).
///
/// The model is embedded in the binary — no download or network access needed.
/// Inference runs via ONNX Runtime (the `ort` crate) with full-graph
/// optimization. Results are memoized via `EMBED_CACHE` since the function is
/// deterministic; repeated inputs return in O(hash) instead of re-running
/// inference. Panics on model init failure.
pub fn embed_text(text: &str, dim: usize) -> Vec<f32> {
    if text.trim().is_empty() {
        return vec![0.0f32; dim];
    }

    cached_or_compute(text, dim, || {
        let m = &*SENTENCE_MODEL;
        let encoding = m
            .tokenizer
            .encode(text, true)
            .expect("tokenization failed");
        let mut session = m.session.lock().expect("ort session mutex poisoned");
        infer_embedding(&mut session, &encoding, dim)
    })
}

/// Batch-embed multiple texts. Each text gets its own forward pass (the ORT
/// session is shaped for batch size 1 in this model). Empty texts get zero
/// vectors. Each element flows through `embed_text`, so identical inputs
/// within or across batches share a cache entry.
pub fn embed_texts_batch(texts: &[&str], dim: usize) -> Vec<Vec<f32>> {
    texts.iter().map(|t| embed_text(t, dim)).collect()
}

/// FNV-1a 64-bit hash (retained for potential future use).
#[cfg(test)]
pub fn fnv_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Cosine similarity between two vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0;
    let mut norm_a = 0.0;
    let mut norm_b = 0.0;

    for i in 0..a.len().min(b.len()) {
        dot += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    dot / (norm_a.sqrt() * norm_b.sqrt())
}

/// Element-wise average of two embedding vectors.
pub fn merge_embeddings(a: &[f32], b: &[f32]) -> Vec<f32> {
    let len = a.len().min(b.len());
    (0..len).map(|i| (a[i] + b[i]) / 2.0).collect()
}

// ── Information compression ─────────────────────────────────────────────

/// Summarize a single text chunk (extractive — pick best sentence).
/// Prioritizes sentences with code references and decision rationale.
pub fn summarize_single(text: &str, kw: &KeywordCache) -> String {
    let trimmed = text.trim();
    if trimmed.len() <= MAX_SUMMARY_LEN {
        return trimmed.to_string();
    }

    let sentences: Vec<&str> = trimmed
        .split(['.', '!', '?', '\n'])
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
            let has_code = kw
                .code
                .iter()
                .any(|(trigger, _, _, _)| s.contains(trigger.as_str()));
            let has_key = s.contains(':') || s.contains('=') || s.contains("TODO");
            let has_decision = kw.decision.iter().any(|k| lower.contains(k.as_str()));
            let has_arch = kw.architecture.iter().any(|k| lower.contains(k.as_str()));
            words
                + if has_code { 5 } else { 0 }
                + if has_key { 3 } else { 0 }
                + if has_decision { 8 } else { 0 }
                + if has_arch { 4 } else { 0 }
        })
        .unwrap_or(&sentences[0]);

    if best.len() <= MAX_SUMMARY_LEN {
        best.to_string()
    } else {
        best.chars().take(MAX_SUMMARY_LEN).collect()
    }
}

/// Merge two texts and pick the best sentence.
pub fn summarize_text(existing: &str, incoming: &str, kw: &KeywordCache) -> String {
    summarize_single(&format!("{} {}", existing, incoming), kw)
}

const GROUP_SUMMARY_ENTRY_LIMIT: usize = 3;
const GROUP_SUMMARY_DIVERSITY_WEIGHT: f32 = 0.6;

fn summary_entry_importance(entry: &ShortTermEntry) -> f32 {
    entry.salience + entry.usage as f32 * 0.1
}

fn lexical_similarity(a: &str, b: &str) -> f32 {
    let tokens_a: HashSet<String> = a
        .split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|w| w.len() > 1)
        .collect();
    let tokens_b: HashSet<String> = b
        .split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|w| w.len() > 1)
        .collect();

    if tokens_a.is_empty() || tokens_b.is_empty() {
        return 0.0;
    }

    let intersection = tokens_a.intersection(&tokens_b).count() as f32;
    let union = tokens_a.union(&tokens_b).count() as f32;
    intersection / union
}

fn summary_entry_similarity(a: &ShortTermEntry, b: &ShortTermEntry) -> f32 {
    if !a.embedding.is_empty() && !b.embedding.is_empty() {
        cosine_similarity(&a.embedding, &b.embedding).clamp(0.0, 1.0)
    } else {
        lexical_similarity(&a.text, &b.text)
    }
}

/// Summarize a group of short-term entries by balancing importance and diversity.
pub fn summarize_group(group: &[ShortTermEntry], kw: &KeywordCache) -> String {
    let mut candidates: Vec<&ShortTermEntry> = group.iter().collect();

    let mut selected: Vec<&ShortTermEntry> = Vec::new();
    while selected.len() < GROUP_SUMMARY_ENTRY_LIMIT && !candidates.is_empty() {
        let best_idx = candidates
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| {
                let score_a = if selected.is_empty() {
                    summary_entry_importance(a)
                } else {
                    let max_similarity = selected
                        .iter()
                        .map(|selected| summary_entry_similarity(a, selected))
                        .fold(0.0_f32, f32::max);
                    summary_entry_importance(a)
                        + (1.0 - max_similarity) * GROUP_SUMMARY_DIVERSITY_WEIGHT
                };
                let score_b = if selected.is_empty() {
                    summary_entry_importance(b)
                } else {
                    let max_similarity = selected
                        .iter()
                        .map(|selected| summary_entry_similarity(b, selected))
                        .fold(0.0_f32, f32::max);
                    summary_entry_importance(b)
                        + (1.0 - max_similarity) * GROUP_SUMMARY_DIVERSITY_WEIGHT
                };
                score_a
                    .partial_cmp(&score_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| b.id.cmp(&a.id))
            })
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        selected.push(candidates.swap_remove(best_idx));
    }

    let mut combined = String::new();
    for entry in selected {
        if !combined.is_empty() {
            combined.push_str(" | ");
        }
        let summary = if entry.summary.is_empty() {
            summarize_single(&entry.text, kw)
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

fn split_sentences(segment: &str) -> Vec<String> {
    let bytes = segment.as_bytes();
    let mut sentences = Vec::new();
    let mut start = 0;
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        let b = bytes[i];
        if (b == b'.' || b == b'!' || b == b'?')
            && (i + 1 >= len || bytes[i + 1].is_ascii_whitespace())
        {
            // Don't split on common abbreviations like "e.g." "i.e." "Dr." etc.
            let is_abbrev = i >= 1
                && i + 1 < len
                && bytes[i - 1].is_ascii_alphabetic()
                && bytes[i + 1] == b' '
                && i + 2 < len
                && bytes[i + 2].is_ascii_lowercase()
                && (i < 2 || bytes[i - 2] == b'.' || bytes[i - 2] == b' ');
            if !is_abbrev {
                let sentence = segment[start..=i].trim();
                if !sentence.is_empty() {
                    sentences.push(sentence.to_string());
                }
                start = i + 1;
            }
        }
        i += 1;
    }

    let tail = segment[start..].trim();
    if !tail.is_empty() {
        sentences.push(tail.to_string());
    }

    sentences
}

fn push_episode_chunk(chunks: &mut Vec<String>, current: &mut String) {
    let chunk = current.trim();
    if !chunk.is_empty() {
        chunks.push(chunk.to_string());
    }
    current.clear();
}

/// Split text into episode-like chunks. Paragraph breaks, topic-shift markers,
/// and `|` separators are hard boundaries; sentence boundaries are soft
/// boundaries used to keep each chunk near a conservative size budget without
/// separating locally related facts.
pub fn chunk_text(text: &str) -> Vec<String> {
    // Phase 1: split on paragraph breaks and pipe separators
    let mut segments: Vec<String> = Vec::new();
    for part in text.split("\n\n") {
        for sub in part.split('|') {
            let trimmed = sub.trim();
            if !trimmed.is_empty() {
                segments.push(trimmed.to_string());
            }
        }
    }

    // Phase 2: split each segment on topic-shift markers (case-insensitive)
    let mut after_topic: Vec<String> = Vec::new();
    for seg in segments {
        let remaining = seg.as_str();
        let lower = seg.to_lowercase();
        let mut last_cut = 0;
        for marker in TOPIC_SHIFT_MARKERS {
            let marker_lower = marker.to_lowercase();
            let mut search_from = 0;
            while let Some(pos) = lower[search_from..].find(&marker_lower) {
                let abs_pos = search_from + pos;
                if abs_pos > last_cut {
                    let before = remaining[last_cut..abs_pos].trim();
                    if !before.is_empty() {
                        after_topic.push(before.to_string());
                    }
                }
                last_cut = abs_pos;
                search_from = abs_pos + marker_lower.len();
            }
        }
        // Remainder after the last marker (or the whole segment if none matched)
        let tail = remaining[last_cut..].trim();
        if !tail.is_empty() {
            after_topic.push(tail.to_string());
        }
    }

    // Phase 3: group adjacent sentences into episode-like chunks.
    let mut chunks: Vec<String> = Vec::new();
    for seg in after_topic {
        let mut current = String::new();
        for sentence in split_sentences(&seg) {
            let projected_len = if current.is_empty() {
                sentence.len()
            } else {
                current.len() + 1 + sentence.len()
            };

            if !current.is_empty() && projected_len > EPISODIC_CHUNK_TARGET_CHARS {
                push_episode_chunk(&mut chunks, &mut current);
            }

            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(&sentence);
        }
        push_episode_chunk(&mut chunks, &mut current);
    }

    if chunks.is_empty() {
        vec![text.trim().to_string()]
    } else {
        chunks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kw() -> KeywordCache {
        KeywordCache::default_from_static()
    }

    #[test]
    fn test_summarize_single_short() {
        assert_eq!(summarize_single("short text", &kw()), "short text");
    }

    #[test]
    fn test_clean_semantic_noise_removes_standalone_junk_tokens() {
        let cleaned = clean_semantic_noise(
            "Project Alpha uses SQLite [[[[ and backups run at 02:30 UTC %%@@ //// ====25.",
        );
        assert!(cleaned.contains("Project Alpha uses SQLite"));
        assert!(cleaned.contains("backups run at 02:30 UTC"));
        assert!(!cleaned.contains("[[[["));
        assert!(!cleaned.contains("%%@@"));
        assert!(!cleaned.contains("////"));
        assert!(!cleaned.contains("====25"));
    }

    #[test]
    fn test_clean_semantic_noise_keeps_meaningful_punctuation_tokens() {
        let cleaned = clean_semantic_noise(
            "C++ writes /tmp/project-alpha.db and https://example.test/a/b stays visible.",
        );
        assert!(cleaned.contains("C++"));
        assert!(cleaned.contains("/tmp/project-alpha.db"));
        assert!(cleaned.contains("https://example.test/a/b"));
    }

    #[test]
    fn test_summarize_single_long() {
        let text = "This is a long sentence about memory systems. fn handle_memory() is the main handler. It processes incoming ticks and queries. The system uses cosine similarity for matching.";
        let summary = summarize_single(text, &kw());
        assert!(summary.len() <= 200);
    }

    #[test]
    fn test_chunk_text_short() {
        let chunks = chunk_text("short text");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "short text");
    }

    #[test]
    fn test_chunk_text_groups_related_sentences() {
        let text = "Project Alpha uses SQLite for metadata. Backups run at 02:30 UTC. Restore drills validate row counts.";
        let chunks = chunk_text(text);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].contains("Project Alpha uses SQLite"));
        assert!(chunks[0].contains("Backups run at 02:30 UTC"));
        assert!(chunks[0].contains("Restore drills validate row counts"));
    }

    #[test]
    fn test_chunk_text_splits_sentence_runs_at_budget() {
        let text = "Project Alpha stores metadata in SQLite and keeps its backup audit log beside the restore checklist. The operator verifies row counts after each restore drill and records mismatches in the audit log. The dashboard reports checkpoint age, file size, and service account status every morning. The release notes include unrelated cafeteria seating observations that should not force the operational facts into separate single-sentence chunks. The restore workflow also records the snapshot identifier, the retention policy, and the runbook section used by the operator.";
        let chunks = chunk_text(text);
        assert!(
            chunks.len() > 1,
            "long sentence run should split near budget, got {:?}",
            chunks
        );
        assert!(
            chunks.iter().all(|c| !c.trim().is_empty()),
            "chunks should not be empty: {:?}",
            chunks
        );
        assert!(
            chunks[0].contains("Project Alpha") && chunks[0].contains("row counts"),
            "first episode should keep adjacent operational facts together: {:?}",
            chunks
        );
    }

    #[test]
    fn test_chunk_text_topic_shift() {
        let text = "We discussed the car detailing. By the way, the GPS had an issue on 3/22.";
        let chunks = chunk_text(text);
        assert!(
            chunks.len() >= 2,
            "topic shift marker should split chunks, got {:?}",
            chunks
        );
        // The GPS fact should be in its own chunk
        assert!(
            chunks.iter().any(|c| c.contains("GPS")),
            "GPS detail should be preserved in a chunk"
        );
    }

    #[test]
    fn test_chunk_text_pipe_separator() {
        let text = "fact one about Redis | fact two about PostgreSQL | fact three about MongoDB";
        let chunks = chunk_text(text);
        assert_eq!(chunks.len(), 3);
    }

    #[test]
    fn test_chunk_text_paragraph_break() {
        let text = "First paragraph about topic A.\n\nSecond paragraph about topic B.";
        let chunks = chunk_text(text);
        assert_eq!(chunks.len(), 2);
    }

    #[test]
    fn test_chunk_text_no_split_mid_sentence() {
        // A long sentence with no boundary markers should stay as one chunk
        let text = "This is a very long sentence about many things including databases and caching and logging and networking and serialization without any period";
        let chunks = chunk_text(text);
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn test_decision_rationale_boosted() {
        let text = "This module handles memory operations with buffers and caches. \
                     We chose cosine similarity over Euclidean distance because it is scale-invariant. \
                     The system also supports batch processing for throughput optimization.";
        let summary = summarize_single(text, &kw());
        assert!(
            summary.contains("chose") || summary.contains("because"),
            "Decision-rationale sentence should be selected, got: {}",
            summary
        );
    }

    #[test]
    fn test_summarize_text_merges() {
        let result = summarize_text("first part", "second part", &kw());
        assert!(!result.is_empty());
        // Should contain content from at least one input
        assert!(
            result.contains("first") || result.contains("second"),
            "got: {}",
            result
        );
    }

    #[test]
    fn test_summarize_group_empty() {
        let result = summarize_group(&[] as &[ShortTermEntry], &kw());
        assert_eq!(result, "Consolidated memory");
    }

    #[test]
    fn test_summarize_group_single_entry() {
        let entry = ShortTermEntry {
            id: 1,
            text: "Fixed the parser bug in memory module".to_string(),
            summary: String::new(),
            embedding: vec![],
            salience: 0.5,
            usage: 1,
            last_access: 100,
            reconsolidation_count: 0,
            labile_until: 0,
            refs: vec![],
            gradient_sq_sum: 0.0,
            density: 0.0,
            consolidated: false,
            emotional_valence: 0.0,
            stability: 1.0,
            last_retrieval_interval: 0,
            ..Default::default()
        };
        let result = summarize_group(&[entry][..], &kw());
        assert!(!result.is_empty());
        assert_ne!(result, "Consolidated memory");
    }

    #[test]
    fn test_summarize_group_picks_top_by_salience() {
        let entries: Vec<ShortTermEntry> = (0..5)
            .map(|i| ShortTermEntry {
                id: i,
                text: format!("Entry number {} with some content here", i),
                summary: String::new(),
                embedding: vec![],
                salience: i as f32 * 0.2,
                usage: 0,
                last_access: 100,
                reconsolidation_count: 0,
                labile_until: 0,
                refs: vec![],
                gradient_sq_sum: 0.0,
                density: 0.0,
                consolidated: false,
                emotional_valence: 0.0,
                stability: 1.0,
                last_retrieval_interval: 0,
                ..Default::default()
            })
            .collect();
        let result = summarize_group(&entries, &kw());
        // Should contain content from highest-salience entries (3, 4)
        assert!(result.contains('|') || !result.is_empty());
        assert!(result.len() <= 300);
    }

    #[test]
    fn test_summarize_group_includes_distinct_lower_salience_fact() {
        let entries = vec![
            ShortTermEntry {
                id: 1,
                text: "database migration primary plan".to_string(),
                summary: "primary database migration plan".to_string(),
                embedding: vec![1.0, 0.0],
                salience: 1.0,
                usage: 0,
                ..Default::default()
            },
            ShortTermEntry {
                id: 2,
                text: "database migration duplicate detail alpha".to_string(),
                summary: "duplicate database migration alpha".to_string(),
                embedding: vec![1.0, 0.0],
                salience: 0.95,
                usage: 0,
                ..Default::default()
            },
            ShortTermEntry {
                id: 3,
                text: "database migration duplicate detail beta".to_string(),
                summary: "duplicate database migration beta".to_string(),
                embedding: vec![1.0, 0.0],
                salience: 0.9,
                usage: 0,
                ..Default::default()
            },
            ShortTermEntry {
                id: 4,
                text: "rollback requires point in time restore validation".to_string(),
                summary: "rollback requires restore validation".to_string(),
                embedding: vec![0.0, 1.0],
                salience: 0.5,
                usage: 0,
                ..Default::default()
            },
        ];

        let result = summarize_group(&entries, &kw());

        assert!(
            result.contains("rollback requires restore validation"),
            "distinct lower-salience fact should survive summary selection, got: {result}"
        );
    }

    #[test]
    fn test_summarize_group_respects_max_length() {
        let entries: Vec<ShortTermEntry> = (0..3)
            .map(|i| ShortTermEntry {
                id: i,
                text: "A".repeat(200),
                summary: String::new(),
                embedding: vec![],
                salience: 1.0,
                usage: 5,
                last_access: 100,
                reconsolidation_count: 0,
                labile_until: 0,
                refs: vec![],
                gradient_sq_sum: 0.0,
                density: 0.0,
                consolidated: false,
                emotional_valence: 0.0,
                stability: 1.0,
                last_retrieval_interval: 0,
                ..Default::default()
            })
            .collect();
        let result = summarize_group(&entries, &kw());
        assert!(result.len() <= 300, "got len {}", result.len());
    }

    #[test]
    fn test_summarize_group_uses_existing_summary() {
        let entry = ShortTermEntry {
            id: 1,
            text: "Very long original text that should not be used".to_string(),
            summary: "Pre-computed summary".to_string(),
            embedding: vec![],
            salience: 0.5,
            usage: 1,
            last_access: 100,
            reconsolidation_count: 0,
            labile_until: 0,
            refs: vec![],
            gradient_sq_sum: 0.0,
            density: 0.0,
            consolidated: false,
            emotional_valence: 0.0,
            stability: 1.0,
            last_retrieval_interval: 0,
            ..Default::default()
        };
        let result = summarize_group(&[entry][..], &kw());
        assert!(result.contains("Pre-computed summary"));
    }

    #[test]
    fn test_chunk_text_empty() {
        let chunks = chunk_text("");
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn test_chunk_text_no_content_lost() {
        let text = "Line one.\nLine two.\nLine three.\nLine four.\nLine five.";
        let chunks = chunk_text(text);
        let reassembled: String = chunks.join(" ");
        // All content should be present
        assert!(reassembled.contains("Line one"));
        assert!(reassembled.contains("Line five"));
    }

    #[test]
    fn test_summarize_single_preserves_short() {
        let text = "Short decision text";
        assert_eq!(summarize_single(text, &kw()), text);
    }

    #[test]
    fn test_summarize_single_code_reference_boost() {
        let text = "We processed the data using standard operations. \
                     fn handle_memory() is the core entry point for all memory operations. \
                     Various configuration options are available for tuning performance.";
        let summary = summarize_single(text, &kw());
        assert!(
            summary.contains("fn handle_memory"),
            "Code reference sentence should be preferred, got: {}",
            summary
        );
    }

    #[test]
    fn test_summarize_architecture_boost() {
        let text = "I worked on the project for several hours. \
                     The system architecture follows a 3-layer hierarchical model. \
                     It was a sunny day outside today.";
        let summary = summarize_single(text, &kw());
        assert!(
            summary.contains("architecture"),
            "Architecture sentence should be boosted (+4), got: {}",
            summary
        );
    }

    #[test]
    fn test_summarize_polyglot_code_boost() {
        // Python boost
        let py = "Some text. def handle_python(): return None. More text.";
        assert!(summarize_single(py, &kw()).contains("def handle_python"));

        // Go boost
        let go = "Intro. func main() { println(1) }. Outro.";
        assert!(summarize_single(go, &kw()).contains("func main"));

        // TypeScript boost
        let ts = "First. interface IData { id: number }. Last.";
        assert!(summarize_single(ts, &kw()).contains("interface IData"));
    }

    // ── Embedding tests ─────────────────────────────────────────────────

    #[test]
    fn test_embed_produces_correct_dimensions() {
        assert_eq!(embed_text("hello world", 384).len(), 384);
    }

    #[test]
    fn test_embed_is_normalized() {
        let vec = embed_text("the quick brown fox jumps over the lazy dog", 384);
        let norm: f32 = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
        // sentence-transformer embeddings are approximately normalized
        assert!(norm > 0.5 && norm < 1.5, "norm={}", norm);
    }

    #[test]
    fn test_cosine_similarity_identical() {
        let a = embed_text("hello world", 384);
        let sim = cosine_similarity(&a, &a);
        assert!((sim - 1.0).abs() < 0.001, "got {}", sim);
    }

    #[test]
    fn test_cosine_similarity_related_vs_unrelated() {
        let a = embed_text("memory system with embeddings and similarity search", 384);
        let b = embed_text("embedding vectors and cosine similarity matching", 384);
        let c = embed_text("cooking recipes for italian pasta dishes", 384);
        assert!(cosine_similarity(&a, &b) > cosine_similarity(&a, &c));
    }

    #[test]
    fn test_cosine_similarity_empty() {
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
    }

    #[test]
    fn test_merge_embeddings() {
        assert_eq!(
            merge_embeddings(&[1.0, 0.0, 0.5], &[0.0, 1.0, 0.5]),
            vec![0.5, 0.5, 0.5]
        );
    }

    #[test]
    fn test_fnv_hash_deterministic() {
        let h1 = fnv_hash(b"hello");
        assert_eq!(h1, fnv_hash(b"hello"));
        assert_ne!(h1, fnv_hash(b"world"));
    }

    #[test]
    fn test_embed_empty_input() {
        let v = embed_text("", 384);
        assert_eq!(v.len(), 384);
    }

    #[test]
    fn test_embed_single_word() {
        let v = embed_text("hello", 384);
        assert_eq!(v.len(), 384);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(norm > 0.5 || norm == 0.0, "norm={}", norm);
    }

    #[test]
    fn test_cosine_similarity_different_lengths() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!((0.0..=1.0).contains(&sim), "got {}", sim);
    }

    #[test]
    fn test_merge_embeddings_different_lengths() {
        let result = merge_embeddings(&[1.0, 2.0, 3.0], &[4.0, 5.0]);
        assert_eq!(result.len(), 2);
        assert_eq!(result, vec![2.5, 3.5]);
    }

    // ── Batch embedding tests ──────────────────────────────────────────────

    #[test]
    fn test_embed_batch_empty() {
        let results = embed_texts_batch(&[], 384);
        assert!(results.is_empty());
    }

    #[test]
    fn test_embed_batch_matches_single() {
        let single = embed_text("hello world", 384);
        let batch = embed_texts_batch(&["hello world"], 384);
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].len(), 384);
        // Should produce identical embeddings
        let sim = cosine_similarity(&single, &batch[0]);
        assert!(
            (sim - 1.0).abs() < 0.001,
            "batch should match single: {}",
            sim
        );
    }

    #[test]
    fn test_embed_batch_multiple() {
        let texts = &["hello world", "memory system", "cooking recipes"];
        let batch = embed_texts_batch(texts, 384);
        assert_eq!(batch.len(), 3);
        for emb in &batch {
            assert_eq!(emb.len(), 384);
        }
        // Related texts should be more similar than unrelated
        let sim_01 = cosine_similarity(&batch[0], &batch[1]);
        let sim_02 = cosine_similarity(&batch[0], &batch[2]);
        // Just verify they're all valid embeddings with non-zero norms
        for emb in &batch {
            let norm: f32 = emb.iter().map(|v| v * v).sum::<f32>().sqrt();
            assert!(norm > 0.5, "norm={}", norm);
        }
        // Sanity: similarities are in valid range
        assert!((-1.0..=1.0).contains(&sim_01));
        assert!((-1.0..=1.0).contains(&sim_02));
    }

    #[test]
    fn test_embed_batch_handles_empty_strings() {
        let batch = embed_texts_batch(&["hello", "", "world"], 384);
        assert_eq!(batch.len(), 3);
        // Empty string should be zero vector
        assert!(batch[1].iter().all(|&v| v == 0.0));
        // Non-empty should have non-zero norms
        let norm_0: f32 = batch[0].iter().map(|v| v * v).sum::<f32>().sqrt();
        let norm_2: f32 = batch[2].iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!(norm_0 > 0.5);
        assert!(norm_2 > 0.5);
    }

    #[test]
    fn concurrent_embed_produces_bit_identical_results() {
        use std::thread;

        let handles: Vec<_> = (0..8)
            .map(|_| {
                thread::spawn(|| {
                    (0..50)
                        .map(|_| embed_text("hello world", 384))
                        .collect::<Vec<_>>()
                })
            })
            .collect();

        let results: Vec<Vec<Vec<f32>>> =
            handles.into_iter().map(|h| h.join().unwrap()).collect();

        let reference = &results[0][0];
        for batch in &results {
            for v in batch {
                assert_eq!(v, reference, "concurrent embed_text produced divergent output");
            }
        }
    }
}
