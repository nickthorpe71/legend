//! GLiNER2 (DeBERTa-v3-small + GLiNER head) fp32 weights, loaded from
//! `models/gliner2-fp32.bin`. The file is produced by the Python
//! exporter at `oracle/03_extract_weights.py`; that script doubles as
//! the format spec.
//!
//! ~582 MB on disk. Behind the `gliner2_fp32` feature flag so the
//! bytes don't blow up production binaries; the long-term plan is an
//! INT8 path (`gliner2-int8.bin`) on the same shape.

use std::sync::LazyLock;

const MAGIC: u32 = 0x4749_4C32;
const FORMAT_VERSION: u32 = 1;

/// Compile-time-baked path to the fp32 weight file. Read at first
/// access of `BUNDLED_DEBERTA_WEIGHTS`. We deliberately avoid
/// `include_bytes!` here because the file is ~582 MB — embedding it
/// inflates every example binary that links `legend` and trips
/// rustc's linker into OOM with LTO. The production GLiNER2 path
/// will be INT8 (~150 MB) and *will* be bundled via `include_bytes!`
/// once Phase 6 lands.
const WEIGHTS_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/models/gliner2-fp32.bin");

/// All weight tensors needed to run a single GLiNER2 forward pass.
/// Shapes are recorded as fields and re-asserted at load time. Every
/// `Vec<f32>` is stored row-major; for `nn.Linear`-style projections
/// the layout is `[in_dim, out_dim]` (transposed from PyTorch's
/// `[out, in]` at export).
#[derive(Debug)]
pub struct WeightsDebertaV3 {
    // ── architecture ─────────────────────────────────────────────
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

    // ── embeddings ───────────────────────────────────────────────
    pub word_emb: Vec<f32>,     // [vocab, hidden]
    pub emb_ln_gamma: Vec<f32>, // [hidden]
    pub emb_ln_beta: Vec<f32>,  // [hidden]

    pub layers: Vec<DebertaLayer>,

    // ── encoder shared (post-stack) ──────────────────────────────
    pub rel_emb: Vec<f32>,          // [2 * position_buckets, hidden]
    pub rel_emb_ln_gamma: Vec<f32>, // [hidden]
    pub rel_emb_ln_beta: Vec<f32>,  // [hidden]
    pub final_ln_gamma: Vec<f32>,   // [hidden]
    pub final_ln_beta: Vec<f32>,    // [hidden]

    // ── 768 → 512 projection ─────────────────────────────────────
    pub proj_w: Vec<f32>, // [hidden, projection_out]
    pub proj_b: Vec<f32>, // [projection_out]

    // ── BiLSTM (1 layer, bidirectional) ──────────────────────────
    pub lstm_fwd: LstmDirection,
    pub lstm_rev: LstmDirection,

    // ── span head (markerV0) ─────────────────────────────────────
    pub project_start: ProjMlp,
    pub project_end: ProjMlp,
    pub out_project: ProjMlp, // input dim = 2 * projection_out

    // ── prompt head ──────────────────────────────────────────────
    pub prompt: ProjMlp,
}

#[derive(Debug)]
pub struct DebertaLayer {
    pub q_w: Vec<f32>, // [hidden, hidden]
    pub q_b: Vec<f32>, // [hidden]
    pub k_w: Vec<f32>,
    pub k_b: Vec<f32>,
    pub v_w: Vec<f32>,
    pub v_b: Vec<f32>,
    pub attn_out_w: Vec<f32>,    // [hidden, hidden]
    pub attn_out_b: Vec<f32>,    // [hidden]
    pub attn_ln_gamma: Vec<f32>, // [hidden]
    pub attn_ln_beta: Vec<f32>,  // [hidden]
    pub ffn_int_w: Vec<f32>,     // [hidden, intermediate]
    pub ffn_int_b: Vec<f32>,     // [intermediate]
    pub ffn_out_w: Vec<f32>,     // [intermediate, hidden]
    pub ffn_out_b: Vec<f32>,     // [hidden]
    pub ffn_ln_gamma: Vec<f32>,  // [hidden]
    pub ffn_ln_beta: Vec<f32>,   // [hidden]
}

/// One direction of the BiLSTM. PyTorch's gate ordering is `(i, f, g, o)`
/// concatenated along the first axis of the weight matrices; the
/// `4*lstm_hidden` dim below preserves that ordering verbatim.
#[derive(Debug)]
pub struct LstmDirection {
    pub ih_w: Vec<f32>, // [input_size, 4 * lstm_hidden]
    pub hh_w: Vec<f32>, // [lstm_hidden, 4 * lstm_hidden]
    pub ih_b: Vec<f32>, // [4 * lstm_hidden]
    pub hh_b: Vec<f32>, // [4 * lstm_hidden]
}

/// The two-layer MLP used by every span / prompt head:
///     out = lin2( relu( lin1(x) ) )
/// `inner_dim` is always `4 * out_dim`. `in_dim` may differ from
/// `out_dim` (for `out_project` the input is the concatenated
/// start+end span vector).
#[derive(Debug)]
pub struct ProjMlp {
    pub lin1_w: Vec<f32>, // [in_dim, inner_dim]
    pub lin1_b: Vec<f32>, // [inner_dim]
    pub lin2_w: Vec<f32>, // [inner_dim, out_dim]
    pub lin2_b: Vec<f32>, // [out_dim]
    pub in_dim: usize,
    pub inner_dim: usize,
    pub out_dim: usize,
}

pub static BUNDLED_DEBERTA_WEIGHTS: LazyLock<WeightsDebertaV3> = LazyLock::new(|| {
    let bytes = std::fs::read(WEIGHTS_PATH)
        .unwrap_or_else(|e| panic!("read {WEIGHTS_PATH}: {e} — regenerate via `oracle/03_extract_weights.py`"));
    WeightsDebertaV3::load_from_bytes(&bytes).expect("failed to parse GLiNER2 fp32 weights")
});

impl WeightsDebertaV3 {
    pub fn load_bundled() -> &'static WeightsDebertaV3 {
        &BUNDLED_DEBERTA_WEIGHTS
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
        let position_buckets = r.u32() as usize;
        let projection_out = r.u32() as usize;
        let max_width = r.u32() as usize;
        let class_token_index = r.u32();
        let num_lstm_layers = r.u32() as usize;
        let layer_norm_eps = r.f32();

        #[allow(clippy::manual_is_multiple_of)]
        if hidden_size % num_heads != 0 {
            return Err(format!(
                "hidden_size {hidden_size} not divisible by num_heads {num_heads}"
            ));
        }
        let head_dim = hidden_size / num_heads;

        // Embeddings.
        let word_emb = r.f32_vec(vocab_size * hidden_size);
        let emb_ln_gamma = r.f32_vec(hidden_size);
        let emb_ln_beta = r.f32_vec(hidden_size);

        // Layers.
        let mut layers = Vec::with_capacity(num_layers);
        for _ in 0..num_layers {
            layers.push(DebertaLayer {
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

        // Encoder shared.
        let rel_emb = r.f32_vec(2 * position_buckets * hidden_size);
        let rel_emb_ln_gamma = r.f32_vec(hidden_size);
        let rel_emb_ln_beta = r.f32_vec(hidden_size);
        let final_ln_gamma = r.f32_vec(hidden_size);
        let final_ln_beta = r.f32_vec(hidden_size);

        // Projection.
        let proj_w = r.f32_vec(hidden_size * projection_out);
        let proj_b = r.f32_vec(projection_out);

        // BiLSTM.
        let lstm_hidden_half = projection_out / 2;
        let mut read_dir = || LstmDirection {
            ih_w: r.f32_vec(projection_out * 4 * lstm_hidden_half),
            hh_w: r.f32_vec(lstm_hidden_half * 4 * lstm_hidden_half),
            ih_b: r.f32_vec(4 * lstm_hidden_half),
            hh_b: r.f32_vec(4 * lstm_hidden_half),
        };
        let lstm_fwd = read_dir();
        let lstm_rev = read_dir();

        let mut read_mlp = |in_dim: usize, out_dim: usize| {
            let inner_dim = 4 * out_dim;
            ProjMlp {
                lin1_w: r.f32_vec(in_dim * inner_dim),
                lin1_b: r.f32_vec(inner_dim),
                lin2_w: r.f32_vec(inner_dim * out_dim),
                lin2_b: r.f32_vec(out_dim),
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
            return Err(format!(
                "trailing {} bytes after parse",
                bytes.len() - r.pos
            ));
        }

        Ok(WeightsDebertaV3 {
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
        let dst = unsafe { std::slice::from_raw_parts_mut(out.as_mut_ptr() as *mut u8, byte_len) };
        dst.copy_from_slice(src);
        self.pos += byte_len;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_weights_parse_with_expected_shape() {
        let w = WeightsDebertaV3::load_bundled();
        assert_eq!(w.num_layers, 6);
        assert_eq!(w.hidden_size, 768);
        assert_eq!(w.num_heads, 12);
        assert_eq!(w.head_dim, 64);
        assert_eq!(w.intermediate_size, 3072);
        assert_eq!(w.vocab_size, 128_004);
        assert_eq!(w.max_position, 512);
        assert_eq!(w.position_buckets, 256);
        assert_eq!(w.projection_out, 512);
        assert_eq!(w.max_width, 12);
        assert_eq!(w.class_token_index, 128_002);
        assert_eq!(w.num_lstm_layers, 1);

        assert_eq!(w.word_emb.len(), 128_004 * 768);
        assert_eq!(w.layers.len(), 6);
        assert_eq!(w.layers[0].q_w.len(), 768 * 768);
        assert_eq!(w.layers[0].ffn_int_w.len(), 768 * 3072);

        assert_eq!(w.rel_emb.len(), 512 * 768);
        assert_eq!(w.proj_w.len(), 768 * 512);
        assert_eq!(w.proj_b.len(), 512);

        assert_eq!(w.lstm_fwd.ih_w.len(), 512 * 1024);
        assert_eq!(w.lstm_fwd.hh_w.len(), 256 * 1024);
        assert_eq!(w.lstm_rev.ih_b.len(), 1024);

        // Span head shapes.
        assert_eq!(w.project_start.in_dim, 512);
        assert_eq!(w.project_start.inner_dim, 2048);
        assert_eq!(w.project_start.out_dim, 512);
        assert_eq!(w.project_start.lin1_w.len(), 512 * 2048);
        assert_eq!(w.out_project.in_dim, 1024);
        assert_eq!(w.out_project.lin1_w.len(), 1024 * 2048);

        // Prompt head.
        assert_eq!(w.prompt.lin1_w.len(), 512 * 2048);
    }
}
