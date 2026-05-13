//! Pure-Rust BERT inference engine for all-MiniLM-L6-v2.
//!
//! Two forward passes:
//! - [`bert::forward`] — fp32 reference (weights bundled as
//!   `models/minilm-fp32.bin`, ~86 MB). Kept as the validation oracle
//!   and as a fallback if INT8 ever diverges suspiciously.
//! - [`bert_int8::forward`] — INT8 quantized (weights bundled as
//!   `models/minilm-int8.bin`, ~22 MB). Symmetric per-output-channel
//!   weight quantization + dynamic per-tensor activation
//!   quantization. SIMD-accelerated INT8 matmul (AVX2 on x86_64,
//!   scalar fallback elsewhere). This is the production path.

pub mod bert_int8;
pub mod ops;
pub mod quantized_ops;
pub mod weights_int8;

// GLiNER2 / DeBERTa-v3 path. INT8 (~150 MB) is the production
// runtime and always available; the fp32 reference path (~582 MB,
// validation only) sits inside `deberta` behind the `gliner2_fp32`
// feature. Both weight files are runtime-loaded so the compiled
// binary is unaffected by their size.
pub mod deberta;

pub use weights_int8::{BUNDLED_WEIGHTS_INT8, WeightsInt8};

// fp32 reference path — feature-gated so production builds don't
// carry the ~86 MB fp32 weight bundle. Enabled by `--features
// fp32_reference` and used by `examples/validate_int8.rs`.
#[cfg(feature = "fp32_reference")]
pub mod attention;
#[cfg(feature = "fp32_reference")]
pub mod bert;
#[cfg(feature = "fp32_reference")]
pub mod encoder;
#[cfg(feature = "fp32_reference")]
pub mod weights;

#[cfg(feature = "fp32_reference")]
pub use bert::forward;
#[cfg(feature = "fp32_reference")]
pub use weights::{BUNDLED_WEIGHTS, Weights};
