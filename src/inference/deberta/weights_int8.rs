//! INT8 GLiNER2 weights. Bundled at `models/gliner2-int8.bin` (~150 MB);
//! see `examples/quantize_gliner2_to_int8.rs` for the producer.
//!
//! Quantization scheme matches the MiniLM INT8 path:
//! - Matmul weights: symmetric per-output-channel INT8, column-major.
//! - Word + relative-position embedding tables: per-tensor INT8.
//! - LayerNorms, biases, and the BiLSTM are kept fp32 — LSTM weights
//!   are small and sequential so quantizing them buys little.

use std::sync::LazyLock;

use crate::inference::weights_int8::{QuantEmbedding, QuantWeight, Reader};

const MAGIC: u32 = 0x4749_4C38;
/// v2 bakes per-column sums into the file (see the comment in
/// `examples/quantize_gliner2_to_int8.rs`). v1 files need to be
/// regenerated with the new quantizer.
const FORMAT_VERSION: u32 = 2;

/// Same path-as-string approach used for the fp32 bundle (so the lib
/// doesn't carry 150 MB of bytes into every consumer's binary). The
/// file is read lazily at first access.
const WEIGHTS_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/models/gliner2-int8.bin");

#[derive(Debug)]
pub struct WeightsDebertaInt8 {
    pub num_layers: usize,
    pub hidden_size: usize,
    pub num_heads: usize,
    pub head_dim: usize,
    pub intermediate_size: usize,
    pub vocab_size: usize,
    pub max_position: usize,
    pub position_buckets: usize,
    pub projection_out: usize,
    pub max_width: usize,
    pub class_token_index: u32,
    pub num_lstm_layers: usize,
    pub layer_norm_eps: f32,

    pub word_emb: QuantEmbedding,
    pub emb_ln_gamma: &'static [f32],
    pub emb_ln_beta: &'static [f32],

    pub layers: Vec<DebertaLayerInt8>,

    pub rel_emb: QuantEmbedding,
    pub rel_emb_ln_gamma: &'static [f32],
    pub rel_emb_ln_beta: &'static [f32],
    pub final_ln_gamma: &'static [f32],
    pub final_ln_beta: &'static [f32],

    pub proj_w: QuantWeight,
    pub proj_b: &'static [f32],

    pub lstm_fwd: LstmDirectionFp32,
    pub lstm_rev: LstmDirectionFp32,

    pub project_start: ProjMlpInt8,
    pub project_end: ProjMlpInt8,
    pub out_project: ProjMlpInt8,
    pub prompt: ProjMlpInt8,
}

#[derive(Debug)]
pub struct DebertaLayerInt8 {
    pub q_w: QuantWeight,
    pub q_b: &'static [f32],
    pub k_w: QuantWeight,
    pub k_b: &'static [f32],
    pub v_w: QuantWeight,
    pub v_b: &'static [f32],
    pub attn_out_w: QuantWeight,
    pub attn_out_b: &'static [f32],
    pub attn_ln_gamma: &'static [f32],
    pub attn_ln_beta: &'static [f32],
    pub ffn_int_w: QuantWeight,
    pub ffn_int_b: &'static [f32],
    pub ffn_out_w: QuantWeight,
    pub ffn_out_b: &'static [f32],
    pub ffn_ln_gamma: &'static [f32],
    pub ffn_ln_beta: &'static [f32],
}

#[derive(Debug)]
pub struct LstmDirectionFp32 {
    pub ih_w: &'static [f32],
    pub hh_w: &'static [f32],
    pub ih_b: &'static [f32],
    pub hh_b: &'static [f32],
}

#[derive(Debug)]
pub struct ProjMlpInt8 {
    pub lin1_w: QuantWeight,
    pub lin1_b: &'static [f32],
    pub lin2_w: QuantWeight,
    pub lin2_b: &'static [f32],
    pub in_dim: usize,
    pub inner_dim: usize,
    pub out_dim: usize,
}

pub static BUNDLED_DEBERTA_INT8: LazyLock<WeightsDebertaInt8> = LazyLock::new(|| {
    // mmap the bundle rather than `fs::read`. Saves the explicit
    // 150 MB Vec<u8> allocation; the kernel demand-faults pages as
    // the parser walks them. We Box::leak the Mmap so its backing
    // storage outlives the LazyLock — the parser later copies bytes
    // out into `Vec<i8>` etc., so the mmap isn't actually borrowed
    // after this function returns, but keeping it leaked is the
    // cleanest way to guarantee live-for-process semantics.
    let file = std::fs::File::open(WEIGHTS_PATH).unwrap_or_else(|e| {
        panic!("open {WEIGHTS_PATH}: {e} — regenerate via `cargo run --release --example quantize_gliner2_to_int8`")
    });
    let mmap = unsafe { memmap2::Mmap::map(&file) }
        .unwrap_or_else(|e| panic!("mmap {WEIGHTS_PATH}: {e}"));
    let leaked: &'static [u8] = Box::leak(Box::new(mmap));
    WeightsDebertaInt8::load_from_bytes(leaked).expect("failed to parse GLiNER2 INT8 weights")
});

impl WeightsDebertaInt8 {
    pub fn load_bundled() -> &'static WeightsDebertaInt8 {
        &BUNDLED_DEBERTA_INT8
    }

    fn load_from_bytes(bytes: &'static [u8]) -> Result<Self, String> {
        let mut r = Reader::new(bytes);
        let magic = r.u32();
        if magic != MAGIC {
            return Err(format!("bad magic: 0x{magic:08x}, expected 0x{MAGIC:08x}"));
        }
        let format_version = r.u32();
        if format_version != FORMAT_VERSION {
            return Err(format!("format version mismatch: {format_version}"));
        }
        let num_layers = r.u32() as usize;
        let hidden_size = r.u32() as usize;
        let num_heads = r.u32() as usize;
        let intermediate_size = r.u32() as usize;
        let vocab_size = r.u32() as usize;
        let max_position = r.u32() as usize;
        let position_buckets = r.u32() as usize;
        let projection_out = r.u32() as usize;
        let max_width = r.u32() as usize;
        let class_token_index = r.u32();
        let num_lstm_layers = r.u32() as usize;
        let layer_norm_eps = r.f32();
        let head_dim = hidden_size / num_heads;

        let word_emb = r.quant_embedding(vocab_size, hidden_size);
        let emb_ln_gamma = r.f32_slice(hidden_size);
        let emb_ln_beta = r.f32_slice(hidden_size);

        let mut layers = Vec::with_capacity(num_layers);
        for _ in 0..num_layers {
            layers.push(DebertaLayerInt8 {
                q_w: r.quant_weight_with_col_sums(hidden_size, hidden_size),
                q_b: r.f32_slice(hidden_size),
                k_w: r.quant_weight_with_col_sums(hidden_size, hidden_size),
                k_b: r.f32_slice(hidden_size),
                v_w: r.quant_weight_with_col_sums(hidden_size, hidden_size),
                v_b: r.f32_slice(hidden_size),
                attn_out_w: r.quant_weight_with_col_sums(hidden_size, hidden_size),
                attn_out_b: r.f32_slice(hidden_size),
                attn_ln_gamma: r.f32_slice(hidden_size),
                attn_ln_beta: r.f32_slice(hidden_size),
                ffn_int_w: r.quant_weight_with_col_sums(hidden_size, intermediate_size),
                ffn_int_b: r.f32_slice(intermediate_size),
                ffn_out_w: r.quant_weight_with_col_sums(intermediate_size, hidden_size),
                ffn_out_b: r.f32_slice(hidden_size),
                ffn_ln_gamma: r.f32_slice(hidden_size),
                ffn_ln_beta: r.f32_slice(hidden_size),
            });
        }

        let rel_emb = r.quant_embedding(2 * position_buckets, hidden_size);
        let rel_emb_ln_gamma = r.f32_slice(hidden_size);
        let rel_emb_ln_beta = r.f32_slice(hidden_size);
        let final_ln_gamma = r.f32_slice(hidden_size);
        let final_ln_beta = r.f32_slice(hidden_size);

        let proj_w = r.quant_weight_with_col_sums(hidden_size, projection_out);
        let proj_b = r.f32_slice(projection_out);

        let lstm_half = projection_out / 2;
        let four_h = 4 * lstm_half;
        let mut read_dir = || LstmDirectionFp32 {
            ih_w: r.f32_slice(projection_out * four_h),
            hh_w: r.f32_slice(lstm_half * four_h),
            ih_b: r.f32_slice(four_h),
            hh_b: r.f32_slice(four_h),
        };
        let lstm_fwd = read_dir();
        let lstm_rev = read_dir();

        let mut read_mlp = |in_dim: usize, out_dim: usize| {
            let inner_dim = 4 * out_dim;
            ProjMlpInt8 {
                lin1_w: r.quant_weight_with_col_sums(in_dim, inner_dim),
                lin1_b: r.f32_slice(inner_dim),
                lin2_w: r.quant_weight_with_col_sums(inner_dim, out_dim),
                lin2_b: r.f32_slice(out_dim),
                in_dim,
                inner_dim,
                out_dim,
            }
        };
        let project_start = read_mlp(projection_out, projection_out);
        let project_end = read_mlp(projection_out, projection_out);
        let out_project = read_mlp(2 * projection_out, projection_out);
        let prompt = read_mlp(projection_out, projection_out);

        if r.pos != bytes.len() {
            return Err(format!("trailing {} bytes", bytes.len() - r.pos));
        }

        Ok(WeightsDebertaInt8 {
            num_layers,
            hidden_size,
            num_heads,
            head_dim,
            intermediate_size,
            vocab_size,
            max_position,
            position_buckets,
            projection_out,
            max_width,
            class_token_index,
            num_lstm_layers,
            layer_norm_eps,
            word_emb,
            emb_ln_gamma,
            emb_ln_beta,
            layers,
            rel_emb,
            rel_emb_ln_gamma,
            rel_emb_ln_beta,
            final_ln_gamma,
            final_ln_beta,
            proj_w,
            proj_b,
            lstm_fwd,
            lstm_rev,
            project_start,
            project_end,
            out_project,
            prompt,
        })
    }
}

// Reader pulled from `crate::inference::weights_int8` — same code,
// same zero-copy semantics, just one less copy.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_int8_loads_with_expected_shape() {
        let w = WeightsDebertaInt8::load_bundled();
        assert_eq!(w.num_layers, 6);
        assert_eq!(w.hidden_size, 768);
        assert_eq!(w.layers.len(), 6);
        assert_eq!(w.layers[0].q_w.q_data.len(), 768 * 768);
        assert_eq!(w.layers[0].q_w.scales.len(), 768);
        assert_eq!(w.layers[0].ffn_int_w.q_data.len(), 768 * 3072);
        assert_eq!(w.rel_emb.q_data.len(), 512 * 768);
        assert_eq!(w.proj_w.q_data.len(), 768 * 512);
        assert_eq!(w.out_project.lin1_w.q_data.len(), 1024 * 2048);
    }
}
