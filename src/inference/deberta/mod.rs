//! Pure-Rust GLiNER2 inference path: DeBERTa-v3-small encoder with
//! disentangled attention, plus the GLiNER head (BiLSTM, span/prompt
//! MLPs, dot-product scoring). Mirrors `src/inference/` for MiniLM but
//! the architecture is different enough — disentangled attention,
//! relative-position bucketing, no absolute position embeddings —
//! that nothing structural ports over verbatim.
//!
//! All weights are bundled via `include_bytes!` from
//! `models/gliner2-fp32.bin`. INT8 quantization is a later phase.

pub mod attention;
pub mod embedding;
pub mod encoder;
pub mod head;
pub mod rel_pos;
pub mod tokenizer;
pub mod weights;

pub use weights::{BUNDLED_DEBERTA_WEIGHTS, DebertaLayer, LstmDirection, ProjMlp, WeightsDebertaV3};
