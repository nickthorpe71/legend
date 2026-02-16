# Building a Brain-Inspired Hierarchical Memory System for LLMs in Rust

You're aiming to create a fast, brain-inspired external memory module in Rust that mimics human memory's layered structure (sensory/immediate → working/short-term → long-term) with dynamic, automatic updates. It should run on every "LLM tick" — either per-token during local inference or per-action/response when using API-based models.

This design draws from:
- **Activation Memory Bank (AMB)** from PaceLLM: similarity-based retrieval, thresholded fusion, LRU eviction, merging of similar entries.
- **HippoRAG**: hippocampal-style graph indexing for deep, transitive retrieval.
- Human memory hierarchy: immediate sensory buffer, limited-capacity working memory, vast consolidated long-term memory with associative recall.
- Hebbian principles: "neurons that fire together wire together" → strengthen frequently co-retrieved connections.

The result is a high-performance, black-box-compatible memory layer that dramatically improves long-context coherence for coding assistants, agents, or chat sessions.

## Core Design Principles

### Memory Layers
1. **Immediate / Sensory Memory**
   - Ultra-short-term buffer of raw recent tokens/chunks.
   - Fixed size (e.g., last 4k-8k tokens).
   - Used directly in the current prompt/context window.

2. **Short-Term / Working Memory**
   - Vector database of embeddings for fast semantic similarity search.
   - Limited capacity (512–4096 slots).
   - Stores compressed representations of recent messages, code snippets, decisions.
   - Retrieval: cosine similarity (like AMB lookup).

3. **Long-Term Memory**
   - Knowledge graph (entities + relations) for structured, multi-hop reasoning.
   - Nodes: functions, classes, variables, concepts, events.
   - Edges: calls, inherits, defines, relates-to, discussed-in.
   - Retrieval: landmark-based + personalized PageRank (HippoRAG style).

### Dynamic Updates (Per Tick)
Inspired by AMB's thresholds and Hebbian learning:
- **Retrieval** → Fuse relevant memories into current context.
- **Update Rules**:
  - High similarity → Reinforce (increment usage counter).
  - Medium similarity → Consolidate (merge/average embedding, strengthen graph edges).
  - Low similarity → Encode new memory (add entry, evict LRU if full).
- Periodic consolidation: summarize short-term clusters into long-term entities/relations.

## Recommended Rust Crates (as of early 2026)

| Layer / Feature              | Crate Recommendations                                                                 | Why It Fits                                                                 |
|------------------------------|---------------------------------------------------------------------------------------|-----------------------------------------------------------------------------|
| Core memory abstraction      | `llm-brain`                                                                           | Explicitly designed as a "memory layer" for LLMs with fragment storage      |
| Vector DB (short-term)       | `nano-vectordb-rs`, `memvdb`, or `lance` (for persistence)                            | Blazing-fast in-memory cosine search, Rayon-parallelized                    |
| Knowledge Graph (long-term)  | `graphrag-rs`, `phago-rag` (Hebbian updates), `petgraph` + custom                  | Full GraphRAG with entity extraction and multi-hop queries                  |
| LLM Inference / Callbacks    | `rustformers/llm`, `candle`, `llama-rs`                                                | Per-token callbacks for true "tick" updates                                 |
| Agent / API Wrapper          | `rig` (rig.rs), `autoagents-rs`                                                       | Easy per-action memory hooks for OpenAI/Anthropic/Groq                       |
| Embeddings                   | `candle` (local), or API calls to embedding endpoints                                 | Fast local or remote embeddings                                             |
| Concurrency                  | `tokio`, `rayon`                                                                      | Async updates without blocking generation                                    |

Search crates.io for the latest versions — the ecosystem moves fast.

## High-Level Architecture

```rust
use std::sync::{Arc, Mutex};
use std::collections::HashMap;

use nano_vectordb_rs::{VectorDB, Embedding}; // Short-term
use graphrag_rs::{KnowledgeGraph, Entity, Relation}; // Long-term
use rustformers::llm::{Model, InferenceCallback, InferenceResponse};

struct BrainMemory {
    immediate: Vec<String>,                    // Sensory buffer
    short_term: VectorDB,                      // Working memory
    long_term: KnowledgeGraph,                 // Consolidated
    usage: HashMap<u64, u32>,                   // Reinforcement counters
}

impl BrainMemory {
    const THETA_HIGH: f32 = 0.95;
    const THETA_LOW: f32 = 0.75;

    fn tick(&mut self, chunk: &str, embedding: Embedding, query_embedding: Option<Embedding>) {
        // 1. Retrieval
        let mut retrieved = String::new();

        if let Some(q_emb) = query_embedding {
            let short_rel = self.short_term.search(&q_emb, top_k=10);
            let long_rel = self.long_term.multi_hop_query(&q_emb);

            // Fuse with thresholds (like AMB)
            for entry in short_rel {
                if entry.similarity > Self::THETA_HIGH {
                    retrieved.push_str(&format!("High-relevance: {}\n", entry.text));
                } else if entry.similarity > Self::THETA_LOW {
                    retrieved.push_str(&format!("Relevant summary: {}\n", entry.summary));
                }
            }
            // Add graph paths...
        }

        // 2. Update after processing chunk
        let sim = self.short_term.max_similarity(&embedding);
        let id = self.short_term.find_closest_id(&embedding);

        match sim {
            s if s > Self::THETA_HIGH => {
                self.usage.entry(id).and_modify(|c| *c += 2); // Strong reinforce
            }
            s if s > Self::THETA_LOW => {
                self.short_term.merge(id, embedding);
                self.long_term.strengthen_related_edges(id); // Hebbian
            }
            _ => {
                if self.short_term.is_full() {
                    let lru = self.least_recently_used();
                    self.short_term.evict(lru);
                }
                let new_id = self.short_term.insert(chunk, embedding);
                self.long_term.extract_and_add_entities(chunk, new_id);
            }
        }

        // Push to immediate
        self.immediate.push(chunk.to_string());
        if self.immediate.len() > 8192 { self.immediate.remove(0); }

        // Return retrieved context for prompt injection
        retrieved
    }
}

// Example per-token inference loop
fn run_with_memory(model: &Model, memory: Arc<Mutex<BrainMemory>>) {
    let callback = |resp: InferenceResponse| {
        let mut mem = memory.lock().unwrap();
        let emb = embed_token(&resp.token); // Batch embeddings periodically
        mem.tick(&resp.token, emb, None);   // Or pass query emb for retrieval
        true
    };

    model.generate_with_callback("Prompt here...", callback);
}

## Build Plan

Step-by-Step Build Guide

Start Simple
cargo new llm-brain-memory
Add llm-brain and nano-vectordb-rs.
Implement just the short-term vector layer with AMB-style thresholds.

Add Per-Tick Hooks
If local model: use rustformers/llm callbacks.
If API: wrap calls in a loop that runs memory.tick() after each response.

Layer in Graph (Long-Term)
Add graphrag-rs.
Periodically (every N ticks) run entity/relation extraction on new short-term entries.
Use LLM (small local model or API) for extraction to keep it automatic.

Dynamic Consolidation
Background task (Tokio spawn) that clusters short-term embeddings and summarizes into long-term nodes.

Integration with Coding Assistant
Wrap your LLM calls: retrieve → build prompt with memory → call LLM → tick with response → repeat.
For code-specific: add parsers (tree-sitter-rs) to extract functions/classes automatically.

Performance Tips
Batch embeddings every 10-50 tokens.
Use Arc<Mutex<>> or dashmap for concurrent access.
Profile with cargo flamegraph.

Testing
Needle-in-haystack style tests: hide info early, query late.
Coding consistency: multi-turn refactors across large virtual codebases.


This architecture can run sub-millisecond per tick on modern hardware. Start with the vector-only version — you'll see huge gains immediately — then layer in the graph for deeper reasoning.