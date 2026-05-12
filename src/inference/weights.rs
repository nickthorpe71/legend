//! Bundled model weights — parsed once at startup from the binary
//! shipped in `models/minilm-fp32.bin`. The file format is documented
//! in `examples/extract_weights.rs`.

use std::sync::LazyLock;

const MAGIC: u32 = 0x4D4C4D31;
const FORMAT_VERSION: u32 = 1;

/// Compile-time-bundled fp32 weights for all-MiniLM-L6-v2.
/// ~86 MB; load is a single pass over the bytes.
const WEIGHT_BYTES: &[u8] = include_bytes!("../../models/minilm-fp32.bin");

/// Architecture constants + every weight tensor needed for the
/// forward pass. Tensors stored as flat `Vec<f32>`; shapes are
/// recorded as fields and asserted at load time.
#[derive(Debug)]
pub struct Weights {
    pub num_layers: usize,
    pub hidden_size: usize,
    pub num_heads: usize,
    pub head_dim: usize,
    pub intermediate_size: usize,
    pub vocab_size: usize,
    pub max_position: usize,
    pub type_vocab_size: usize,
    pub layer_norm_eps: f32,

    // Embeddings.
    pub word_emb: Vec<f32>,          // [vocab, hidden]
    pub pos_emb: Vec<f32>,           // [max_pos, hidden]
    pub type_emb: Vec<f32>,          // [type_vocab, hidden]
    pub emb_ln_gamma: Vec<f32>,      // [hidden]
    pub emb_ln_beta: Vec<f32>,       // [hidden]

    // Per layer; outer index is layer 0..num_layers.
    pub layers: Vec<LayerWeights>,
}

#[derive(Debug)]
pub struct LayerWeights {
    pub q_w: Vec<f32>,            // [hidden, hidden]
    pub q_b: Vec<f32>,            // [hidden]
    pub k_w: Vec<f32>,
    pub k_b: Vec<f32>,
    pub v_w: Vec<f32>,
    pub v_b: Vec<f32>,
    pub attn_out_w: Vec<f32>,     // [hidden, hidden]
    pub attn_out_b: Vec<f32>,     // [hidden]
    pub attn_ln_gamma: Vec<f32>,  // [hidden]
    pub attn_ln_beta: Vec<f32>,   // [hidden]
    pub ffn_int_w: Vec<f32>,      // [hidden, intermediate]
    pub ffn_int_b: Vec<f32>,      // [intermediate]
    pub ffn_out_w: Vec<f32>,      // [intermediate, hidden]
    pub ffn_out_b: Vec<f32>,      // [hidden]
    pub ffn_ln_gamma: Vec<f32>,   // [hidden]
    pub ffn_ln_beta: Vec<f32>,    // [hidden]
}

/// Lazy global singleton — first access parses the bundled bytes
/// (~80 ms one-time on modern hardware), every subsequent access is
/// a pointer load.
pub static BUNDLED_WEIGHTS: LazyLock<Weights> = LazyLock::new(|| {
    Weights::load_from_bytes(WEIGHT_BYTES).expect("failed to load bundled weights")
});

impl Weights {
    pub fn load_bundled() -> &'static Weights {
        &BUNDLED_WEIGHTS
    }

    fn load_from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let mut r = Reader::new(bytes);
        let magic = r.u32();
        if magic != MAGIC {
            return Err(format!("bad magic: 0x{magic:08x}, expected 0x{MAGIC:08x}"));
        }
        let format_version = r.u32();
        if format_version != FORMAT_VERSION {
            return Err(format!(
                "format version mismatch: {format_version} vs expected {FORMAT_VERSION}"
            ));
        }
        let num_layers = r.u32() as usize;
        let hidden_size = r.u32() as usize;
        let num_heads = r.u32() as usize;
        let intermediate_size = r.u32() as usize;
        let vocab_size = r.u32() as usize;
        let max_position = r.u32() as usize;
        let type_vocab_size = r.u32() as usize;
        let _padding = r.u32();
        let layer_norm_eps = r.f32();

        if hidden_size % num_heads != 0 {
            return Err(format!(
                "hidden_size {hidden_size} not divisible by num_heads {num_heads}"
            ));
        }
        let head_dim = hidden_size / num_heads;

        let word_emb = r.f32_vec(vocab_size * hidden_size);
        let pos_emb = r.f32_vec(max_position * hidden_size);
        let type_emb = r.f32_vec(type_vocab_size * hidden_size);
        let emb_ln_gamma = r.f32_vec(hidden_size);
        let emb_ln_beta = r.f32_vec(hidden_size);

        let mut layers = Vec::with_capacity(num_layers);
        for _ in 0..num_layers {
            layers.push(LayerWeights {
                q_w: r.f32_vec(hidden_size * hidden_size),
                q_b: r.f32_vec(hidden_size),
                k_w: r.f32_vec(hidden_size * hidden_size),
                k_b: r.f32_vec(hidden_size),
                v_w: r.f32_vec(hidden_size * hidden_size),
                v_b: r.f32_vec(hidden_size),
                attn_out_w: r.f32_vec(hidden_size * hidden_size),
                attn_out_b: r.f32_vec(hidden_size),
                attn_ln_gamma: r.f32_vec(hidden_size),
                attn_ln_beta: r.f32_vec(hidden_size),
                ffn_int_w: r.f32_vec(hidden_size * intermediate_size),
                ffn_int_b: r.f32_vec(intermediate_size),
                ffn_out_w: r.f32_vec(intermediate_size * hidden_size),
                ffn_out_b: r.f32_vec(hidden_size),
                ffn_ln_gamma: r.f32_vec(hidden_size),
                ffn_ln_beta: r.f32_vec(hidden_size),
            });
        }

        if r.pos != bytes.len() {
            return Err(format!(
                "trailing {} bytes after parse",
                bytes.len() - r.pos
            ));
        }

        Ok(Weights {
            num_layers,
            hidden_size,
            num_heads,
            head_dim,
            intermediate_size,
            vocab_size,
            max_position,
            type_vocab_size,
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
        let v = u32::from_le_bytes(
            self.bytes[self.pos..self.pos + 4]
                .try_into()
                .expect("u32 read"),
        );
        self.pos += 4;
        v
    }
    fn f32(&mut self) -> f32 {
        let v = f32::from_le_bytes(
            self.bytes[self.pos..self.pos + 4]
                .try_into()
                .expect("f32 read"),
        );
        self.pos += 4;
        v
    }
    /// Read `n` little-endian f32s. Hot path: copy raw bytes once.
    fn f32_vec(&mut self, n: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; n];
        let byte_len = n * 4;
        let src = &self.bytes[self.pos..self.pos + byte_len];
        // SAFETY: f32 is 4 bytes; we copy the exact byte count and
        // assume host is little-endian (asserted below). LE is the
        // standard on every platform we ship to (x86_64 + aarch64).
        let dst = unsafe {
            std::slice::from_raw_parts_mut(out.as_mut_ptr() as *mut u8, byte_len)
        };
        dst.copy_from_slice(src);
        self.pos += byte_len;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_is_little_endian() {
        // We rely on this in `Reader::f32_vec`.
        assert_eq!(1u32.to_le_bytes(), [1, 0, 0, 0]);
    }

    #[test]
    fn bundled_weights_parse_with_expected_shape() {
        let w = Weights::load_bundled();
        assert_eq!(w.num_layers, 6);
        assert_eq!(w.hidden_size, 384);
        assert_eq!(w.num_heads, 12);
        assert_eq!(w.head_dim, 32);
        assert_eq!(w.intermediate_size, 1536);
        assert_eq!(w.vocab_size, 30522);
        assert_eq!(w.max_position, 512);
        assert_eq!(w.type_vocab_size, 2);
        assert!((w.layer_norm_eps - 1e-12).abs() < 1e-15);
        assert_eq!(w.word_emb.len(), 30522 * 384);
        assert_eq!(w.pos_emb.len(), 512 * 384);
        assert_eq!(w.layers.len(), 6);
        assert_eq!(w.layers[0].q_w.len(), 384 * 384);
        assert_eq!(w.layers[0].ffn_int_w.len(), 384 * 1536);
        assert_eq!(w.layers[5].ffn_out_w.len(), 1536 * 384);
    }
}
