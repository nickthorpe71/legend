//! Pure-Rust GLiNER2 inference path: DeBERTa-v3-small encoder with
//! disentangled attention, plus the GLiNER head (BiLSTM, span/prompt
//! MLPs, dot-product scoring). The INT8 path is the production runtime
//! and is always compiled; the fp32 path is feature-gated under
//! `gliner2_fp32` and only used by the oracle-validation tests in
//! `tests/deberta_oracle.rs`.
//!
//! Both paths load their weights from `models/gliner2-int8.bin`
//! (always) and `models/gliner2-fp32.bin` (only when fp32 is needed).
//! The files are runtime-loaded — neither inflates compiled binaries.

// Always-on. The INT8 path is the production runtime; pure-logic
// helpers (decoding, preprocessor, rel-pos buckets, tokenizer) sit
// here too so Step 5 can call them without the feature flag.
pub mod decoding;
pub mod forward_int8;
pub mod predict;
pub mod preprocess;
pub mod rel_pos;
pub mod tokenizer;
pub mod weights_int8;

// fp32 reference path. Used by the oracle validator and the
// layer-by-layer test fixtures; not on the production path.
#[cfg(feature = "gliner2_fp32")]
pub mod attention;
#[cfg(feature = "gliner2_fp32")]
pub mod embedding;
#[cfg(feature = "gliner2_fp32")]
pub mod encoder;
#[cfg(feature = "gliner2_fp32")]
pub mod head;
#[cfg(feature = "gliner2_fp32")]
pub mod weights;

#[cfg(feature = "gliner2_fp32")]
pub use weights::{BUNDLED_DEBERTA_WEIGHTS, DebertaLayer, LstmDirection, ProjMlp, WeightsDebertaV3};
