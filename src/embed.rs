use std::sync::LazyLock;

use tokenizers::Tokenizer;
use tract_onnx::prelude::*;

/// Output dimensionality of the bundled all-MiniLM-L6-v2 quantized model.
/// Single source of truth — callers allocate buffers and arrays against this
/// constant. If the bundled model is ever swapped for one with a different
/// hidden size, this changes in lockstep.
pub const EMBEDDING_DIM: usize = 384;

/// Compute a semantic embedding vector using all-MiniLM-L6-v2 quantized (384-dim).
///
/// The model is embedded in the binary — no download or network access needed.
/// Inference runs via tract-onnx (pure Rust). Panics on model init failure.
pub fn embed_text(text: &str) -> Vec<f32> {
    if text.trim().is_empty() {
        return vec![0.0f32; EMBEDDING_DIM];
    }
    let m = &*SENTENCE_MODEL;
    let encoding = m.tokenizer.encode(text, true).expect("tokenization failed");
    infer_embedding(&m.model, &encoding)
}

// Tract's runnable inference plan. Verbose generic — alias for readability.
type RunnableModel = SimplePlan<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>;

// MiniLM model bundle — tokenizer plus the runnable plan. tract's `SimplePlan::run`
// takes `&self`, so unlike ORT we don't need a Mutex; multiple threads can run
// inference concurrently against the shared plan.
struct SentenceModel {
    tokenizer: Tokenizer,
    model: RunnableModel,
}

// Lazy-initialized sentence embedding model (all-MiniLM-L6-v2 quantized, 384-dim).
// The model is embedded directly in the binary — no network download required.
// Inference runs via the pure-Rust tract-onnx crate; no C++ deps, portable to
// any OS Rust supports.
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
    // input doesn't produce a tensor the model can't handle.
    let _ = tokenizer.with_truncation(Some(tokenizers::TruncationParams {
        max_length: 512,
        ..Default::default()
    }));

    let onnx_bytes: &[u8] = include_bytes!("../models/all-MiniLM-L6-v2-q/model.onnx");
    let model = tract_onnx::onnx()
        .model_for_read(&mut std::io::Cursor::new(onnx_bytes))
        .expect("read onnx model")
        .into_optimized()
        .expect("optimize onnx model")
        .into_runnable()
        .expect("make model runnable");

    SentenceModel { tokenizer, model }
});

// Run inference on a single tokenizer encoding and return the L2-normalized
// attention-masked mean-pooled embedding. Mean pooling matches how
// all-MiniLM-L6-v2 was trained and used by sentence-transformers — CLS
// pooling underperforms on sentence-similarity benchmarks for this model.
fn infer_embedding(model: &RunnableModel, encoding: &tokenizers::Encoding) -> Vec<f32> {
    let input_ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
    let attention_mask: Vec<i64> = encoding
        .get_attention_mask()
        .iter()
        .map(|&m| m as i64)
        .collect();
    let token_type_ids: Vec<i64> = encoding.get_type_ids().iter().map(|&t| t as i64).collect();
    let seq_len = input_ids.len();

    // Build [1, seq_len] tensors of i64 — BERT-family ONNX models expect this shape.
    let ids_tensor: Tensor = tract_ndarray::Array2::from_shape_vec((1, seq_len), input_ids)
        .expect("ids array shape")
        .into();
    let mask_tensor: Tensor = tract_ndarray::Array2::from_shape_vec((1, seq_len), attention_mask)
        .expect("mask array shape")
        .into();
    let types_tensor: Tensor = tract_ndarray::Array2::from_shape_vec((1, seq_len), token_type_ids)
        .expect("types array shape")
        .into();

    let outputs = model
        .run(tvec!(
            ids_tensor.into(),
            mask_tensor.into(),
            types_tensor.into()
        ))
        .expect("tract inference failed");

    // MiniLM-style models emit "last_hidden_state" as output 0. tract returns
    // outputs in declared order, so index 0 is correct regardless of name.
    let output = outputs[0].to_array_view::<f32>().expect("output is f32");
    let shape = output.shape();
    let hidden_size = shape[shape.len() - 1];
    debug_assert_eq!(
        hidden_size, EMBEDDING_DIM,
        "model output dim != EMBEDDING_DIM"
    );

    // Shape: [1, seq_len, hidden_size]. Mean-pool over non-padding tokens,
    // weighted by the attention mask, matching the reference sentence-
    // transformers implementation.
    let view = output.as_slice().expect("output is contiguous");
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
