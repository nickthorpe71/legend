//! INT8-quantized model weights. See
//! `examples/quantize_to_int8.rs` for the file format and quantization
//! scheme (symmetric, per-output-channel for matmul weights, per-tensor
//! for embeddings, column-major layout for weights so SIMD reads
//! contiguous columns).
//!
//! Weights live in `models/minilm-int8.bin` (~22 MB) and are bundled
//! into the binary via `include_bytes!`.

use std::sync::LazyLock;

const MAGIC: u32 = 0x4D4C4D38;
const FORMAT_VERSION: u32 = 1;

const WEIGHT_BYTES: &[u8] = include_bytes!("../../models/minilm-int8.bin");

/// One quantized 2D weight tensor + its per-output-channel scales.
/// Data is **column-major**: column j occupies `q_data[j*in_dim ..
/// (j+1)*in_dim]` contiguously.
///
/// `col_sums[j] = Σ_k q_data[j*in_dim + k]` — per-column sum of the
/// i8 weights, precomputed at load. Used by the VNNI matmul to
/// correct the `+128` activation shift via `Σ a_u8 * w_i8 = (i8-acc)
/// + 128 * col_sum`.
#[derive(Debug)]
pub struct QuantWeight {
    pub q_data: Vec<i8>,
    pub scales: Vec<f32>,
    pub col_sums: Vec<i32>,
    pub in_dim: usize,
    pub out_dim: usize,
}

/// Per-tensor quantized embedding table. Stored row-major; scale is a
/// single f32. Dequantization is `fp32 = q * scale` (symmetric, no
/// zero-point).
#[derive(Debug)]
pub struct QuantEmbedding {
    pub q_data: Vec<i8>,
    pub scale: f32,
    pub rows: usize,
    pub cols: usize,
}

#[derive(Debug)]
pub struct WeightsInt8 {
    pub num_layers: usize,
    pub hidden_size: usize,
    pub num_heads: usize,
    pub head_dim: usize,
    pub intermediate_size: usize,
    pub vocab_size: usize,
    pub max_position: usize,
    pub type_vocab_size: usize,
    pub layer_norm_eps: f32,

    pub word_emb: QuantEmbedding,
    pub pos_emb: QuantEmbedding,
    pub type_emb: QuantEmbedding,
    pub emb_ln_gamma: Vec<f32>,
    pub emb_ln_beta: Vec<f32>,

    pub layers: Vec<LayerWeightsInt8>,
}

#[derive(Debug)]
pub struct LayerWeightsInt8 {
    pub q_w: QuantWeight,
    pub q_b: Vec<f32>,
    pub k_w: QuantWeight,
    pub k_b: Vec<f32>,
    pub v_w: QuantWeight,
    pub v_b: Vec<f32>,
    pub attn_out_w: QuantWeight,
    pub attn_out_b: Vec<f32>,
    pub attn_ln_gamma: Vec<f32>,
    pub attn_ln_beta: Vec<f32>,
    pub ffn_int_w: QuantWeight,
    pub ffn_int_b: Vec<f32>,
    pub ffn_out_w: QuantWeight,
    pub ffn_out_b: Vec<f32>,
    pub ffn_ln_gamma: Vec<f32>,
    pub ffn_ln_beta: Vec<f32>,
}

pub static BUNDLED_WEIGHTS_INT8: LazyLock<WeightsInt8> = LazyLock::new(|| {
    WeightsInt8::load_from_bytes(WEIGHT_BYTES).expect("failed to load INT8 weights")
});

impl WeightsInt8 {
    pub fn load_bundled() -> &'static WeightsInt8 {
        &BUNDLED_WEIGHTS_INT8
    }

    fn load_from_bytes(bytes: &[u8]) -> Result<Self, String> {
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
        let hidden = r.u32() as usize;
        let num_heads = r.u32() as usize;
        let intermediate = r.u32() as usize;
        let vocab = r.u32() as usize;
        let max_pos = r.u32() as usize;
        let type_vocab = r.u32() as usize;
        let _padding = r.u32();
        let layer_norm_eps = r.f32();

        let head_dim = hidden / num_heads;

        let word_emb = r.quant_embedding(vocab, hidden);
        let pos_emb = r.quant_embedding(max_pos, hidden);
        let type_emb = r.quant_embedding(type_vocab, hidden);
        let emb_ln_gamma = r.f32_vec(hidden);
        let emb_ln_beta = r.f32_vec(hidden);

        let mut layers = Vec::with_capacity(num_layers);
        for _ in 0..num_layers {
            layers.push(LayerWeightsInt8 {
                q_w: r.quant_weight(hidden, hidden),
                q_b: r.f32_vec(hidden),
                k_w: r.quant_weight(hidden, hidden),
                k_b: r.f32_vec(hidden),
                v_w: r.quant_weight(hidden, hidden),
                v_b: r.f32_vec(hidden),
                attn_out_w: r.quant_weight(hidden, hidden),
                attn_out_b: r.f32_vec(hidden),
                attn_ln_gamma: r.f32_vec(hidden),
                attn_ln_beta: r.f32_vec(hidden),
                ffn_int_w: r.quant_weight(hidden, intermediate),
                ffn_int_b: r.f32_vec(intermediate),
                ffn_out_w: r.quant_weight(intermediate, hidden),
                ffn_out_b: r.f32_vec(hidden),
                ffn_ln_gamma: r.f32_vec(hidden),
                ffn_ln_beta: r.f32_vec(hidden),
            });
        }

        if r.pos != bytes.len() {
            return Err(format!("trailing {} bytes", bytes.len() - r.pos));
        }

        Ok(WeightsInt8 {
            num_layers,
            hidden_size: hidden,
            num_heads,
            head_dim,
            intermediate_size: intermediate,
            vocab_size: vocab,
            max_position: max_pos,
            type_vocab_size: type_vocab,
            layer_norm_eps,
            word_emb,
            pos_emb,
            type_emb,
            emb_ln_gamma,
            emb_ln_beta,
            layers,
        })
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
    fn u32(&mut self) -> u32 {
        let v = u32::from_le_bytes(self.bytes[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        v
    }
    fn f32(&mut self) -> f32 {
        let v = f32::from_le_bytes(self.bytes[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        v
    }
    fn f32_vec(&mut self, n: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; n];
        let byte_len = n * 4;
        let src = &self.bytes[self.pos..self.pos + byte_len];
        let dst = unsafe {
            std::slice::from_raw_parts_mut(out.as_mut_ptr() as *mut u8, byte_len)
        };
        dst.copy_from_slice(src);
        self.pos += byte_len;
        out
    }
    fn i8_vec(&mut self, n: usize) -> Vec<i8> {
        let mut out = vec![0i8; n];
        let src = &self.bytes[self.pos..self.pos + n];
        let dst = unsafe { std::slice::from_raw_parts_mut(out.as_mut_ptr() as *mut u8, n) };
        dst.copy_from_slice(src);
        self.pos += n;
        out
    }
    fn quant_embedding(&mut self, rows: usize, cols: usize) -> QuantEmbedding {
        let q_data = self.i8_vec(rows * cols);
        let scale = self.f32();
        QuantEmbedding {
            q_data,
            scale,
            rows,
            cols,
        }
    }
    fn quant_weight(&mut self, in_dim: usize, out_dim: usize) -> QuantWeight {
        // Stored column-major: out_dim columns of in_dim contiguous i8s.
        let q_data = self.i8_vec(in_dim * out_dim);
        let scales = self.f32_vec(out_dim);
        // Precompute per-column sums for VNNI shift correction.
        let mut col_sums = vec![0i32; out_dim];
        for j in 0..out_dim {
            let col = &q_data[j * in_dim..(j + 1) * in_dim];
            col_sums[j] = col.iter().map(|&v| v as i32).sum();
        }
        QuantWeight {
            q_data,
            scales,
            col_sums,
            in_dim,
            out_dim,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_int8_weights_have_expected_shape() {
        let w = WeightsInt8::load_bundled();
        assert_eq!(w.num_layers, 6);
        assert_eq!(w.hidden_size, 384);
        assert_eq!(w.num_heads, 12);
        assert_eq!(w.head_dim, 32);
        assert_eq!(w.intermediate_size, 1536);
        assert_eq!(w.vocab_size, 30522);
        assert_eq!(w.layers.len(), 6);
        assert_eq!(w.layers[0].q_w.q_data.len(), 384 * 384);
        assert_eq!(w.layers[0].q_w.scales.len(), 384);
        assert_eq!(w.layers[0].ffn_int_w.q_data.len(), 384 * 1536);
        assert_eq!(w.layers[0].ffn_int_w.scales.len(), 1536);
        assert_eq!(w.word_emb.q_data.len(), 30522 * 384);
    }
}
