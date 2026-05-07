use std::sync::{LazyLock, Mutex};

use ort::session::{Session, builder::GraphOptimizationLevel};
use ort::value::Tensor;
use tokenizers::Tokenizer;

/// Output dimensionality of the bundled all-MiniLM-L6-v2 quantized model.
/// Single source of truth — callers allocate buffers and arrays against this
/// constant. If the bundled model is ever swapped for one with a different
/// hidden size, this changes in lockstep.
pub const EMBEDDING_DIM: usize = 384;

/// Compute a semantic embedding vector using all-MiniLM-L6-v2 quantized (384-dim).
///
/// The model is embedded in the binary — no download or network access needed.
/// Inference runs via ONNX Runtime (the `ort` crate) with full-graph
/// optimization. Panics on model init failure.
pub fn embed_text(text: &str) -> Vec<f32> {
    if text.trim().is_empty() {
        return vec![0.0f32; EMBEDDING_DIM];
    }
    let m = &*SENTENCE_MODEL;
    let encoding = m.tokenizer.encode(text, true).expect("tokenization failed");
    let mut session = m.session.lock().expect("ort session mutex poisoned");
    infer_embedding(&mut session, &encoding)
}

// MiniLM model keeps its tokenizer (thread-safe via `&self`) and an ONNX
// Runtime session. `ort::Session::run` takes `&mut self` in rc.12, so we wrap
// it in a `Mutex` — the ORT backend is already thread-parallel internally, so
// serializing callers at the mutex doesn't cost us the SIMD/kernel parallelism
// that's the whole reason to use ORT. The daemon holds this singleton and
// amortizes the ~300-500 ms Session init across every tick for the rest of
// the session lifetime.
struct SentenceModel {
    tokenizer: Tokenizer,
    session: Mutex<Session>,
}

// Lazy-initialized sentence embedding model (all-MiniLM-L6-v2 quantized, 384-dim).
// The model is embedded directly in the binary — no network download required.
// Inference runs via ONNX Runtime (the `ort` crate) with graph optimizations
// enabled. ORT ships as a dynamic library; the `ort` crate's `download-binaries`
// feature fetches the right build for the current target at `cargo build` time.
//
// # Model choice
// We use all-MiniLM-L6-v2 quantized (~23MB) over BAAI/bge-small-en-v1.5 (~45MB)
// for faster inference (6 layers vs 12) and smaller binary size. Both produce
// 384-dim embeddings.
static SENTENCE_MODEL: LazyLock<SentenceModel> = LazyLock::new(|| {
    let tokenizer_bytes: &[u8] = include_bytes!("../models/all-MiniLM-L6-v2-q/tokenizer.json");
    let mut tokenizer =
        Tokenizer::from_bytes(tokenizer_bytes).expect("Failed to load embedded tokenizer");
    // Cap inputs at the model's 512-token max-position-embedding so a long
    // input doesn't produce a tensor the model can't handle. Tokenizer
    // returns `&mut Self` from `with_truncation`; we discard it.
    let _ = tokenizer.with_truncation(Some(tokenizers::TruncationParams {
        max_length: 512,
        ..Default::default()
    }));

    let onnx_bytes: &[u8] = include_bytes!("../models/all-MiniLM-L6-v2-q/model.onnx");
    let cpu_core_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .max(1);
    let session = Session::builder()
        .expect("ort session builder")
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .expect("ort set optimization level")
        .with_intra_threads(cpu_core_count)
        .expect("ort set intra threads")
        .commit_from_memory(onnx_bytes)
        .expect("ort commit from memory (ONNX model bytes)");

    SentenceModel {
        tokenizer,
        session: Mutex::new(session),
    }
});

// Run inference on a single tokenizer encoding and return the L2-normalized
// attention-masked mean-pooled embedding. Mean pooling matches how
// all-MiniLM-L6-v2 was trained and used by sentence-transformers — CLS
// pooling underperforms on sentence-similarity benchmarks for this model.
//
// The caller holds the session mutex for the duration of `run()`. ORT's
// intra-op thread pool parallelizes each op across cores, so serializing at
// the mutex doesn't cost us the CPU parallelism that made us pick ORT in the
// first place.
fn infer_embedding(session: &mut Session, encoding: &tokenizers::Encoding) -> Vec<f32> {
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
        Tensor::from_array(([1, seq_len], attention_mask)).expect("mask tensor");
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
    let (_, output_value) = outputs
        .iter()
        .next()
        .expect("ONNX inference produced no outputs");

    let tensor = output_value
        .try_extract_array::<f32>()
        .expect("output tensor is not f32");
    let shape = tensor.shape();
    let hidden_size = shape[shape.len() - 1];
    debug_assert_eq!(hidden_size, EMBEDDING_DIM, "model output dim != EMBEDDING_DIM");

    // Shape: [1, seq_len, hidden_size]. Mean-pool over non-padding tokens,
    // weighted by the attention mask, matching the reference sentence-
    // transformers implementation. We read the attention mask straight from
    // the encoding (avoids cloning the i64 vec we already moved into the
    // tensor).
    let view = tensor.as_slice().expect("output tensor is not contiguous");
    let raw_mask = encoding.get_attention_mask();

    let mut pooled = vec![0.0f32; hidden_size];
    let mut total_mask = 0.0f32;
    for (mask, row) in raw_mask.iter().zip(view.chunks(hidden_size)) {
        let m = *mask as f32;
        if m == 0.0 {
            continue;
        }
        total_mask += m;
        for (p, &v) in pooled.iter_mut().zip(row.iter()) {
            *p += v * m;
        }
    }

    // Fused divide-by-mask-total + L2-normalize: two passes over `pooled`
    // instead of three. First pass divides by the mask total and accumulates
    // sum-of-squares; second pass divides by the resulting L2 norm.
    let denom = total_mask.max(1e-9);
    let mut sum_sq = 0.0f32;
    for x in pooled.iter_mut() {
        *x /= denom;
        sum_sq += *x * *x;
    }
    let norm = sum_sq.sqrt() + 1e-12;
    for x in pooled.iter_mut() {
        *x /= norm;
    }

    pooled
}
