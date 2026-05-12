//! Quantize the bundled fp32 weights (`models/minilm-fp32.bin`) to
//! symmetric INT8 and write `models/minilm-int8.bin`. Pure Rust, no
//! tract / ONNX involvement — operates on the already-extracted fp32
//! format.
//!
//! Run: `cargo run --release --example quantize_to_int8`
//!
//! ── Scheme ───────────────────────────────────────────────────────
//! **Symmetric quantization, no zero_points.** For each tensor T:
//!   scale = max(|T|) / 127.0
//!   T_q   = round(T / scale).clamp(-127, 127) as i8
//!   T_fp32_recovered = T_q * scale
//!
//! **Granularity:**
//! - Matmul weights ([in, out]): **per-output-channel** scales — one
//!   f32 scale per column j, computed over the in×1 slice T[:, j].
//!   Best accuracy for matmul weights, which often have very different
//!   magnitudes per output channel.
//! - Embedding tables: **per-tensor** scale — a single f32 over the
//!   whole table. Embedding lookups don't benefit from per-channel
//!   granularity (we copy a single row at a time at runtime).
//! - Biases & LayerNorm params: **kept fp32**. Tiny in size, used in
//!   the fp32 dequant + add path, not in any matmul.
//!
//! ── Output format (`models/minilm-int8.bin`) ──────────────────────
//!
//! All little-endian. Sizes match the fp32 format's header layout but
//! the magic differs so we can't accidentally cross-load.
//!
//! Header (10 × u32 + 1 × f32) — identical fields to fp32 except magic:
//!   magic                u32  0x4D4C4D38 ("MLM8")
//!   format_version       u32  1
//!   num_layers           u32  6
//!   hidden_size          u32  384
//!   num_heads            u32  12
//!   intermediate_size    u32  1536
//!   vocab_size           u32  30522
//!   max_position         u32  512
//!   type_vocab_size      u32  2
//!   _padding             u32  0
//!   layer_norm_eps       f32  1e-12
//!
//! Embeddings (per-tensor, scalar scale follows each i8 tensor):
//!   word_emb_q           [vocab_size, hidden_size] i8
//!   word_emb_scale       f32 (scalar)
//!   pos_emb_q            [max_position, hidden_size] i8
//!   pos_emb_scale        f32 (scalar)
//!   type_emb_q           [type_vocab_size, hidden_size] i8
//!   type_emb_scale       f32 (scalar)
//!   emb_ln_gamma         [hidden_size] f32
//!   emb_ln_beta          [hidden_size] f32
//!
//! Per layer (6×):
//!   q_w_q                [hidden, hidden] i8
//!   q_w_scale            [hidden] f32                 (per-output-channel)
//!   q_b                  [hidden] f32
//!   k_w_q                [hidden, hidden] i8
//!   k_w_scale            [hidden] f32
//!   k_b                  [hidden] f32
//!   v_w_q                [hidden, hidden] i8
//!   v_w_scale            [hidden] f32
//!   v_b                  [hidden] f32
//!   attn_out_w_q         [hidden, hidden] i8
//!   attn_out_w_scale     [hidden] f32
//!   attn_out_b           [hidden] f32
//!   attn_ln_gamma        [hidden] f32
//!   attn_ln_beta         [hidden] f32
//!   ffn_int_w_q          [hidden, intermediate] i8
//!   ffn_int_w_scale      [intermediate] f32
//!   ffn_int_b            [intermediate] f32
//!   ffn_out_w_q          [intermediate, hidden] i8
//!   ffn_out_w_scale      [hidden] f32
//!   ffn_out_b            [hidden] f32
//!   ffn_ln_gamma         [hidden] f32
//!   ffn_ln_beta          [hidden] f32

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

const INT8_MAGIC: u32 = 0x4D4C4D38;
const FP32_MAGIC: u32 = 0x4D4C4D31;
const FORMAT_VERSION: u32 = 1;

fn main() -> std::io::Result<()> {
    let fp32_path = Path::new("models/minilm-fp32.bin");
    let int8_path = Path::new("models/minilm-int8.bin");

    let bytes = std::fs::read(fp32_path)?;
    let mut r = Reader::new(&bytes);

    // ── Header ────────────────────────────────────────────────────
    let magic = r.u32();
    assert_eq!(magic, FP32_MAGIC, "expected fp32 magic at start");
    let format_version = r.u32();
    assert_eq!(format_version, FORMAT_VERSION);
    let num_layers = r.u32();
    let hidden_size = r.u32();
    let num_heads = r.u32();
    let intermediate_size = r.u32();
    let vocab_size = r.u32();
    let max_position = r.u32();
    let type_vocab_size = r.u32();
    let _padding = r.u32();
    let layer_norm_eps = r.f32();

    let mut out = BufWriter::new(File::create(int8_path)?);
    write_u32(&mut out, INT8_MAGIC)?;
    write_u32(&mut out, FORMAT_VERSION)?;
    write_u32(&mut out, num_layers)?;
    write_u32(&mut out, hidden_size)?;
    write_u32(&mut out, num_heads)?;
    write_u32(&mut out, intermediate_size)?;
    write_u32(&mut out, vocab_size)?;
    write_u32(&mut out, max_position)?;
    write_u32(&mut out, type_vocab_size)?;
    write_u32(&mut out, 0)?;
    write_f32(&mut out, layer_norm_eps)?;

    let h = hidden_size as usize;
    let inter = intermediate_size as usize;

    // ── Embeddings ────────────────────────────────────────────────
    let word_emb = r.f32_vec((vocab_size * hidden_size) as usize);
    let pos_emb = r.f32_vec((max_position * hidden_size) as usize);
    let type_emb = r.f32_vec((type_vocab_size * hidden_size) as usize);
    let emb_ln_gamma = r.f32_vec(h);
    let emb_ln_beta = r.f32_vec(h);

    write_quantized_per_tensor(&mut out, &word_emb)?;
    write_quantized_per_tensor(&mut out, &pos_emb)?;
    write_quantized_per_tensor(&mut out, &type_emb)?;
    write_f32_slice(&mut out, &emb_ln_gamma)?;
    write_f32_slice(&mut out, &emb_ln_beta)?;

    // ── Per layer ─────────────────────────────────────────────────
    for layer in 0..num_layers as usize {
        // weight, bias pairs — weights get per-channel INT8, biases stay fp32.
        let q_w = r.f32_vec(h * h);
        let q_b = r.f32_vec(h);
        let k_w = r.f32_vec(h * h);
        let k_b = r.f32_vec(h);
        let v_w = r.f32_vec(h * h);
        let v_b = r.f32_vec(h);
        let attn_out_w = r.f32_vec(h * h);
        let attn_out_b = r.f32_vec(h);
        let attn_ln_g = r.f32_vec(h);
        let attn_ln_b = r.f32_vec(h);
        let ffn_int_w = r.f32_vec(h * inter);
        let ffn_int_b = r.f32_vec(inter);
        let ffn_out_w = r.f32_vec(inter * h);
        let ffn_out_b = r.f32_vec(h);
        let ffn_ln_g = r.f32_vec(h);
        let ffn_ln_b = r.f32_vec(h);

        // 4 attention matmuls: [h, h] each, per-channel over out=h
        write_quantized_per_channel(&mut out, &q_w, h, h)?;
        write_f32_slice(&mut out, &q_b)?;
        write_quantized_per_channel(&mut out, &k_w, h, h)?;
        write_f32_slice(&mut out, &k_b)?;
        write_quantized_per_channel(&mut out, &v_w, h, h)?;
        write_f32_slice(&mut out, &v_b)?;
        write_quantized_per_channel(&mut out, &attn_out_w, h, h)?;
        write_f32_slice(&mut out, &attn_out_b)?;
        write_f32_slice(&mut out, &attn_ln_g)?;
        write_f32_slice(&mut out, &attn_ln_b)?;
        // FFN intermediate: [h, inter], per-channel over out=inter
        write_quantized_per_channel(&mut out, &ffn_int_w, h, inter)?;
        write_f32_slice(&mut out, &ffn_int_b)?;
        // FFN output: [inter, h], per-channel over out=h
        write_quantized_per_channel(&mut out, &ffn_out_w, inter, h)?;
        write_f32_slice(&mut out, &ffn_out_b)?;
        write_f32_slice(&mut out, &ffn_ln_g)?;
        write_f32_slice(&mut out, &ffn_ln_b)?;

        eprintln!("layer {layer} quantized");
    }

    out.flush()?;
    let bytes = std::fs::metadata(int8_path)?.len();
    println!(
        "wrote {} ({:.1} MB)",
        int8_path.display(),
        bytes as f64 / 1_048_576.0
    );

    Ok(())
}

// ── Quantization helpers ─────────────────────────────────────────────

/// Per-tensor symmetric INT8 quantization.
/// `scale = max(|t|) / 127.0`, `q = round(t / scale).clamp(-127, 127)`.
/// Writes the i8 buffer followed by the scalar f32 scale.
fn write_quantized_per_tensor<W: Write>(w: &mut W, t: &[f32]) -> std::io::Result<()> {
    let max_abs = t.iter().copied().map(f32::abs).fold(0.0f32, f32::max);
    let scale = (max_abs / 127.0).max(1e-12);
    let q: Vec<i8> = t.iter().map(|&v| quantize_one(v, scale)).collect();
    write_i8_slice(w, &q)?;
    write_f32(w, scale)
}

/// Per-output-channel symmetric INT8 quantization. Input tensor shape
/// is `[in, out]` row-major (matches the fp32 file's layout); we
/// transpose during quantization so the output i8 buffer is
/// **column-major**: each column j is `in_dim` contiguous bytes.
/// This is the layout the SIMD matmul kernel reads.
///
/// Produces one f32 scale per output column.
fn write_quantized_per_channel<W: Write>(
    w: &mut W,
    t: &[f32],
    in_dim: usize,
    out_dim: usize,
) -> std::io::Result<()> {
    assert_eq!(t.len(), in_dim * out_dim);
    let mut scales = vec![0.0f32; out_dim];
    // Pass 1: per-column max absolute value.
    for i in 0..in_dim {
        let row = &t[i * out_dim..(i + 1) * out_dim];
        for (j, &v) in row.iter().enumerate() {
            let av = v.abs();
            if av > scales[j] {
                scales[j] = av;
            }
        }
    }
    for s in &mut scales {
        *s = (*s / 127.0).max(1e-12);
    }
    // Pass 2: quantize AND transpose. Output layout is column-major:
    // q[j * in_dim + i] = quantize(t[i * out_dim + j]).
    let mut q = vec![0i8; t.len()];
    for j in 0..out_dim {
        for i in 0..in_dim {
            q[j * in_dim + i] = quantize_one(t[i * out_dim + j], scales[j]);
        }
    }
    write_i8_slice(w, &q)?;
    write_f32_slice(w, &scales)
}

fn quantize_one(v: f32, scale: f32) -> i8 {
    let q = (v / scale).round();
    q.clamp(-127.0, 127.0) as i8
}

// ── Byte plumbing ────────────────────────────────────────────────────

fn write_u32<W: Write>(w: &mut W, v: u32) -> std::io::Result<()> {
    w.write_all(&v.to_le_bytes())
}
fn write_f32<W: Write>(w: &mut W, v: f32) -> std::io::Result<()> {
    w.write_all(&v.to_le_bytes())
}
fn write_f32_slice<W: Write>(w: &mut W, slice: &[f32]) -> std::io::Result<()> {
    let bytes: &[u8] = unsafe {
        std::slice::from_raw_parts(slice.as_ptr() as *const u8, slice.len() * 4)
    };
    w.write_all(bytes)
}
fn write_i8_slice<W: Write>(w: &mut W, slice: &[i8]) -> std::io::Result<()> {
    let bytes: &[u8] = unsafe {
        std::slice::from_raw_parts(slice.as_ptr() as *const u8, slice.len())
    };
    w.write_all(bytes)
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
}
