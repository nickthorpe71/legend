use super::{
    dentate_gyrus::word_overlap,
    entorhinal::cosine_similarity,
    entorhinal::embed_text,
    entorhinal::{summarize_single, summarize_text},
    wernicke::{self, extract_entities},
    BrainState, CONSOLIDATED_EVICTION_REDUCTION, EVICTION_DECAY_RATE, HIPPOCAMPAL_DECAY_RATE,
    KEYWORD_MATCH_BONUS, KEYWORD_MATCH_BONUS_CAP, MIN_QUERY_SIMILARITY, PRUNE_AGE_WEIGHT,
    PRUNE_THRESHOLD, PRUNE_USAGE_WEIGHT, RECONSOLIDATION_THRESHOLD,
};
/// Hippocampus — Episodic memory (L2) functions.
///
/// The hippocampus is the brain's episodic memory system, responsible for encoding,
/// storing, and retrieving specific experiences. In Legend, this corresponds to the
/// `short_term` vector store (`Vec<ShortTermEntry>`):
///
/// - **Encoding**: New ticks that pass the prefrontal attention gate are stored as
///   L2 entries with embeddings, salience, and emotional valence.
/// - **Reconsolidation**: Retrieved memories enter a labile window during which
///   related new information can update them in-place (rather than creating duplicates).
/// - **Pattern completion (CA3)**: Partial cues activate graph structure to reconstruct
///   full memories, modeling the hippocampal CA3 autoassociative network.
/// - **Replay (sharp-wave ripples)**: During consolidation, temporally co-active entries
///   reinforce shared graph edges, modeling hippocampal replay during offline periods.
/// - **Forgetting curve**: Salience decays exponentially, modulated by Ebbinghaus-style
///   stability that grows with spaced retrieval.
///
/// Core types (`ShortTermEntry`, `MemoryRef`) remain in `mod.rs` for serialization.
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Hippocampal types
// ---------------------------------------------------------------------------

/// A single entry in the short-term vector store.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ShortTermEntry {
    pub id: u64,
    pub text: String,
    #[serde(default)]
    pub summary: String,
    pub embedding: Vec<f32>,
    pub last_access: u64,
    pub usage: u32,
    #[serde(default)]
    pub salience: f32,
    /// How many times this entry has been reconsolidated (retrieved then updated).
    #[serde(default)]
    pub reconsolidation_count: u32,
    /// Clock tick until which this entry is labile (editable after retrieval).
    /// Zero means stable.
    #[serde(default)]
    pub labile_until: u64,
    /// Source references (file + line range) associated with this memory.
    #[serde(default)]
    pub refs: Vec<MemoryRef>,
    /// Accumulated squared gradient for AdaGrad adaptive learning rate.
    #[serde(default)]
    pub gradient_sq_sum: f32,
    /// Semantic density: weighted count of high-signal entities (CodeSymbols, FilePaths).
    #[serde(default)]
    pub density: f32,
    /// Whether this entry has been consolidated into the long-term graph.
    /// Consolidated entries are filtered from query results to avoid redundancy.
    #[serde(default)]
    pub consolidated: bool,
    /// Amygdala emotional valence: negative = threat, positive = reward.
    /// Decays at half the hippocampal rate, modeling emotional persistence.
    #[serde(default)]
    pub emotional_valence: f32,
    /// Ebbinghaus stability: how resistant this memory is to decay.
    /// Starts at 1.0, grows with spaced retrieval (capped at 10.0).
    /// Higher stability → slower forgetting curve.
    #[serde(default = "default_stability")]
    pub stability: f32,
    /// Clock interval between the two most recent retrievals.
    /// Used to detect spaced vs massed retrieval patterns.
    #[serde(default)]
    pub last_retrieval_interval: u64,
}

impl Default for ShortTermEntry {
    fn default() -> Self {
        Self {
            id: 0,
            text: String::new(),
            summary: String::new(),
            embedding: Vec::new(),
            last_access: 0,
            usage: 0,
            salience: 0.0,
            reconsolidation_count: 0,
            labile_until: 0,
            refs: Vec::new(),
            gradient_sq_sum: 0.0,
            density: 0.0,
            consolidated: false,
            emotional_valence: 0.0,
            stability: 1.0,
            last_retrieval_interval: 0,
        }
    }
}

/// A source reference to a file region for this memory.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct MemoryRef {
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    /// Short snippet for re-anchoring when lines drift.
    #[serde(default)]
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySnippet {
    pub id: u64,
    pub text: String,
    pub similarity: f32,
    #[serde(default)]
    pub refs: Vec<MemoryRef>,
}

/// Maximum source references per memory entry.
pub const MAX_REFS_PER_ENTRY: usize = 8;

/// Default stability for new entries (Ebbinghaus forgetting curve).
pub fn default_stability() -> f32 {
    1.0
}

// ---------------------------------------------------------------------------
// Narrow-param functions (operate on specific fields, not full BrainState)
// ---------------------------------------------------------------------------

/// Find the short-term entry most similar to the given embedding.
/// Returns (entry_id, similarity). Returns (0, -1.0) if store is empty.
pub fn find_best_match(short_term: &[ShortTermEntry], embedding: &[f32]) -> (u64, f32) {
    short_term
        .iter()
        .fold((0, -1.0_f32), |(best_id, best_sim), entry| {
            let sim = cosine_similarity(&entry.embedding, embedding);
            if sim > best_sim {
                (entry.id, sim)
            } else {
                (best_id, best_sim)
            }
        })
}

/// Return the top-k most similar short-term entries to the given embedding.
/// When `query` is provided, non-stopword keywords that appear in entry text
/// receive a small similarity bonus to improve lexical precision.
pub fn top_k_similar(
    short_term: &[ShortTermEntry],
    embedding: &[f32],
    k: usize,
    query: &str,
) -> Vec<MemorySnippet> {
    // Pre-compute query keywords (lowercased, non-stopword, len > 1)
    let query_keywords: Vec<String> = query
        .split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|w| w.len() > 1 && !wernicke::is_stopword(w))
        .collect();

    let mut scored: Vec<MemorySnippet> = short_term
        .iter()
        .filter(|e| !e.consolidated)
        .map(|e| {
            let cosine = cosine_similarity(&e.embedding, embedding);
            let keyword_bonus = if !query_keywords.is_empty() {
                let entry_lower = e.text.to_lowercase();
                let matches = query_keywords
                    .iter()
                    .filter(|kw| entry_lower.contains(kw.as_str()))
                    .count();
                (matches as f32 * KEYWORD_MATCH_BONUS).min(KEYWORD_MATCH_BONUS_CAP)
            } else {
                0.0
            };
            // Amygdala boost: emotionally charged memories surface more readily
            let emotional_boost = e.emotional_valence.abs() * 0.05;
            MemorySnippet {
                id: e.id,
                text: e.text.clone(),
                similarity: cosine + keyword_bonus + emotional_boost,
                refs: e.refs.clone(),
            }
        })
        .collect();
    scored.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());
    scored.truncate(k);
    scored.retain(|s| s.similarity >= MIN_QUERY_SIMILARITY);
    scored
}

/// Clear labile state from entries whose labile window has expired.
pub fn stabilize_labile_entries(short_term: &mut [ShortTermEntry], clock: u64) {
    for entry in short_term.iter_mut() {
        if entry.labile_until > 0 && entry.labile_until < clock {
            entry.labile_until = 0;
        }
    }
}

/// Remove short-term entries whose composite score has fallen below threshold.
pub fn prune_short_term(short_term: &mut Vec<ShortTermEntry>, clock: u64) {
    short_term.retain(|entry| {
        let age = clock.saturating_sub(entry.last_access) as f32;
        entry.salience + (entry.usage as f32 * PRUNE_USAGE_WEIGHT) - (age * PRUNE_AGE_WEIGHT)
            > PRUNE_THRESHOLD
    });
}

/// L2 exponential decay: salience decays modulated by density and Ebbinghaus stability.
/// Emotional valence decays at half rate (amygdala persistence).
pub fn apply_l2_decay(short_term: &mut [ShortTermEntry], clock: u64) {
    for entry in short_term.iter_mut() {
        // Semantic density reduces decay rate. High-density entries (many symbols/paths) persist longer.
        let density_factor = (1.0 + entry.density * 0.1).min(2.0);
        let base_decay_rate = HIPPOCAMPAL_DECAY_RATE / density_factor;

        // Ebbinghaus: stability slows the forgetting curve. Higher stability → slower decay.
        let effective_decay_rate = base_decay_rate / entry.stability;

        let age = clock.saturating_sub(entry.last_access) as f32;
        let decay = (-age * effective_decay_rate).exp();
        entry.salience *= decay;

        // Amygdala: emotional valence decays at half rate (emotional memories persist longer)
        let emotional_decay = (-age * effective_decay_rate * 0.5).exp();
        entry.emotional_valence *= emotional_decay;
    }
}

// ---------------------------------------------------------------------------
// Wide-param functions (operate on full BrainState)
// ---------------------------------------------------------------------------

/// Try to reconsolidate a new tick into an existing labile memory.
/// Returns the target entry ID if reconsolidation occurred.
pub fn try_reconsolidate(
    state: &mut BrainState,
    text: &str,
    embedding: &[f32],
    salience: f32,
    refs: Vec<MemoryRef>,
) -> Option<u64> {
    let now = state.clock;

    // Find the best labile match
    let mut best: Option<(u64, f32)> = None;
    for entry in &state.short_term {
        if entry.labile_until < now {
            continue; // not labile
        }
        let sim = cosine_similarity(&entry.embedding, embedding);
        let overlap = word_overlap(&entry.text, text);
        // Reconsolidation requires meaningful relation but lower bar than merge
        if sim >= RECONSOLIDATION_THRESHOLD && overlap >= 0.1 {
            let score = sim * 0.6 + overlap * 0.4;
            if best.is_none_or(|(_, best_score)| score > best_score) {
                best = Some((entry.id, score));
            }
        }
    }

    let (target_id, _) = best?;

    // Perform reconsolidation: update the entry in-place
    if let Some(entry) = state.short_term.iter_mut().find(|e| e.id == target_id) {
        // Merge text: append new information
        let merged_text = format!("{} | {}", entry.text, text);
        entry.summary = summarize_text(&entry.text, text, &state.keyword_cache);
        entry.text = if merged_text.len() > 500 {
            // If text is getting too long, use summary as text
            entry.summary.clone()
        } else {
            merged_text
        };
        // Re-embed with combined text
        entry.embedding = embed_text(&entry.text, state.config.embedding_dim);
        // Boost salience (reconsolidated memories are important)
        entry.salience = (entry.salience + salience * 0.3).min(1.0);
        entry.usage = entry.usage.saturating_add(1);
        entry.last_access = now;
        entry.reconsolidation_count += 1;
        entry.density = calculate_density(&entry.text, &state.keyword_cache);
        // Re-stabilize: no longer labile
        entry.labile_until = 0;
        merge_memory_refs(&mut entry.refs, refs);

        return Some(target_id);
    }

    None
}

/// Insert a new short-term entry, evicting the lowest-scoring entry if at capacity.
pub fn insert_short_term(
    state: &mut BrainState,
    text: &str,
    embedding: Vec<f32>,
    salience: f32,
    refs: Vec<MemoryRef>,
    emotional_valence: f32,
) {
    if state.short_term.len() >= state.config.short_term_capacity {
        let now = state.clock;
        // Systems consolidation: consolidated entries whose Summary node
        // has a valid embedding in L3 get reduced eviction resistance,
        // since the neocortex can independently serve their role.
        let has_embedded_summary = |entry: &ShortTermEntry| -> bool {
            entry.consolidated
                && state.long_term.nodes.values().any(|n| {
                    n.kind == "Summary"
                        && !n.embedding.is_empty()
                        && n.source_texts.iter().any(|st| st == &entry.text)
                })
        };
        if let Some(idx) = state
            .short_term
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let mut score_a = eviction_score(a, now);
                let mut score_b = eviction_score(b, now);
                if has_embedded_summary(a) {
                    score_a -= CONSOLIDATED_EVICTION_REDUCTION;
                }
                if has_embedded_summary(b) {
                    score_b -= CONSOLIDATED_EVICTION_REDUCTION;
                }
                score_a.partial_cmp(&score_b).unwrap()
            })
            .map(|(i, _)| i)
        {
            state.short_term.remove(idx);
        }
    }

    let mut refs = refs;
    if refs.len() > MAX_REFS_PER_ENTRY {
        refs.truncate(MAX_REFS_PER_ENTRY);
    }

    state.short_term.push(ShortTermEntry {
        id: state.next_id,
        text: text.to_string(),
        summary: summarize_single(text, &state.keyword_cache),
        embedding,
        last_access: state.clock,
        usage: 1,
        salience: salience.clamp(0.0, 1.0),
        reconsolidation_count: 0,
        labile_until: 0,
        refs,
        gradient_sq_sum: 0.0,
        density: calculate_density(text, &state.keyword_cache),
        consolidated: false,
        emotional_valence,
        stability: 1.0,
        last_retrieval_interval: 0,
    });
    state.next_id += 1;
}

/// CA3 pattern completion: use graph structure to reconstruct related memories
/// from partial cues.
pub fn pattern_complete(
    state: &BrainState,
    query: &str,
    partial_matches: &[MemorySnippet],
    query_mode: super::neocortex::QueryMode,
) -> Vec<MemorySnippet> {
    // Collect entity seeds from both the query and partial matches
    let mut seed_ids: Vec<u64> = Vec::new();
    let query_entities = extract_entities(query, &state.keyword_cache);
    for entity in &query_entities {
        if let Some(&id) = state.long_term.index.get(&entity.label) {
            seed_ids.push(id);
        }
    }
    for snippet in partial_matches {
        let entities = extract_entities(&snippet.text, &state.keyword_cache);
        for entity in &entities {
            if let Some(&id) = state.long_term.index.get(&entity.label) {
                seed_ids.push(id);
            }
        }
    }
    seed_ids.sort();
    seed_ids.dedup();

    if seed_ids.is_empty() {
        return Vec::new();
    }

    // Spread activation to find related graph nodes
    let activated =
        super::neocortex::spreading_activation(&state.long_term, &seed_ids, 2, 0.5, query_mode);

    // Collect source_texts from activated nodes
    let mut candidate_texts: Vec<(String, f32)> = Vec::new();
    for (nid, activation) in &activated {
        if let Some(node) = state.long_term.nodes.get(nid) {
            for st in &node.source_texts {
                candidate_texts.push((st.clone(), *activation));
            }
        }
    }

    // Search L2 for entries containing any candidate text
    let existing_ids: HashSet<u64> = partial_matches.iter().map(|s| s.id).collect();
    let mut completed: Vec<MemorySnippet> = Vec::new();

    for entry in &state.short_term {
        if existing_ids.contains(&entry.id) || entry.consolidated {
            continue;
        }
        // Check if this entry's text matches any activated source_text
        let best_activation = candidate_texts
            .iter()
            .filter(|(text, _)| entry.text.contains(text.as_str()) || text.contains(&entry.text))
            .map(|(_, act)| *act)
            .fold(0.0_f32, f32::max);

        if best_activation > 0.0 {
            // Blend: use entry's own embedding similarity as a factor
            let query_emb = embed_text(query, state.config.embedding_dim);
            let direct_sim = cosine_similarity(&entry.embedding, &query_emb);
            let completion_score = direct_sim * 0.6 + best_activation * 0.4;

            completed.push(MemorySnippet {
                id: entry.id,
                text: entry.text.clone(),
                similarity: completion_score,
                refs: entry.refs.clone(),
            });
        }
    }

    completed.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());
    completed.truncate(3);
    completed
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Composite eviction score: higher = more worth keeping.
///
/// Balances salience (40%), usage (30%), recency (30%), plus emotional
/// resistance from amygdala valence.
pub fn eviction_score(entry: &ShortTermEntry, now: u64) -> f32 {
    let age = now.saturating_sub(entry.last_access) as f32;
    let recency = (-age * EVICTION_DECAY_RATE).exp();
    let usage = (entry.usage as f32).ln_1p();
    // Amygdala: emotionally charged memories resist eviction
    let emotional_resistance = entry.emotional_valence.abs() * 0.15;
    entry.salience * 0.4 + usage * 0.3 + recency * 0.3 + emotional_resistance
}

/// Semantic density: weighted count of high-signal entities in text.
///
/// Used to prioritize code-rich entries during eviction — entries with
/// file paths and code symbols are harder to reconstruct and more valuable.
pub fn calculate_density(text: &str, kw: &super::wernicke::KeywordCache) -> f32 {
    let entities = super::wernicke::extract_entities(text, kw);
    let mut score = 0.0;
    for entity in entities {
        score += match entity.kind.as_str() {
            "FilePath" => 1.0,
            "Function" | "Struct" | "Enum" | "Trait" | "Class" => 0.8,
            "Symbol" | "Type" => 0.4,
            _ => 0.05,
        };
    }
    score
}

/// Merge incoming source references into existing, deduplicating by (path, start, end).
pub fn merge_memory_refs(existing: &mut Vec<MemoryRef>, incoming: Vec<MemoryRef>) {
    if incoming.is_empty() {
        return;
    }

    let mut seen: HashSet<(String, usize, usize)> = existing
        .iter()
        .map(|r| (r.path.clone(), r.start_line, r.end_line))
        .collect();

    for reference in incoming {
        let key = (
            reference.path.clone(),
            reference.start_line,
            reference.end_line,
        );
        if seen.insert(key) {
            existing.push(reference);
        }
        if existing.len() >= MAX_REFS_PER_ENTRY {
            break;
        }
    }

    if existing.len() > MAX_REFS_PER_ENTRY {
        existing.truncate(MAX_REFS_PER_ENTRY);
    }
}

/// Extract file:line source references from tick text.
///
/// Recognizes patterns like `path/to/file.rs#L42` or `file.rs#L10-20`.
pub fn extract_memory_refs_from_text(text: &str) -> Vec<MemoryRef> {
    let mut refs = Vec::new();
    let mut seen: HashSet<(String, usize, usize)> = HashSet::new();

    for line in text.lines() {
        let snippet = build_ref_snippet(line);
        for token in line.split_whitespace() {
            if let Some(reference) = parse_memory_ref_token(token, &snippet) {
                let key = (
                    reference.path.clone(),
                    reference.start_line,
                    reference.end_line,
                );
                if seen.insert(key) {
                    refs.push(reference);
                }
                if refs.len() >= MAX_REFS_PER_ENTRY {
                    return refs;
                }
            }
        }
    }

    refs
}

/// Parse a single token for a file:line reference pattern.
fn parse_memory_ref_token(token: &str, snippet: &str) -> Option<MemoryRef> {
    let trimmed =
        token.trim_matches(|c: char| matches!(c, '`' | '"' | '\'' | ',' | '.' | ')' | ']' | ';'));
    let (path_part, line_part) = trimmed.split_once("#L")?;
    let path = path_part.trim_matches(|c: char| matches!(c, '`' | '"' | '\'' | '(' | '['));
    if path.is_empty() {
        return None;
    }

    let cleaned = line_part.trim_matches(|c: char| matches!(c, ',' | '.' | ')' | ']' | ';'));
    if cleaned.is_empty() {
        return None;
    }

    let (start_line, end_line) = if let Some((start_s, end_s)) = cleaned.split_once('-') {
        let start_line: usize = start_s.parse().ok()?;
        let end_line: usize = end_s.parse().ok()?;
        if end_line >= start_line {
            (start_line, end_line)
        } else {
            (end_line, start_line)
        }
    } else {
        let line: usize = cleaned.parse().ok()?;
        (line, line)
    };

    Some(MemoryRef {
        path: path.to_string(),
        start_line,
        end_line,
        snippet: snippet.to_string(),
    })
}

/// Build a short snippet from a line for re-anchoring when line numbers drift.
fn build_ref_snippet(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.len() <= 120 {
        trimmed.to_string()
    } else {
        trimmed.chars().take(120).collect()
    }
}
