//! Quantize the fp32 GLiNER2 weights (`models/gliner2-fp32.bin`) to
//! symmetric INT8 and write `models/gliner2-int8.bin`. Mirror of
//! `examples/quantize_to_int8.rs`, extended for the DeBERTa-v3 +
//! GLiNER head shapes.
//!
//! Run: `cargo run --release --example quantize_gliner2_to_int8`
//!
//! ── Scheme ───────────────────────────────────────────────────────
//! Same as MiniLM:
//! - Matmul weights: per-output-channel symmetric INT8, column-major.
//! - Embedding tables: per-tensor symmetric INT8.
//! - Biases + LayerNorm + (for now) LSTM weights stay fp32.
//!
//! ── Output format (`models/gliner2-int8.bin`) ────────────────────
//! Header identical to fp32 except magic 0x47494C38 ("GLN8"). Then
//! tensors in the same order as the fp32 file, with matmul weights
//! replaced by `(i8 column-major buffer, per-output-channel scales)`
//! and embedding tables replaced by `(i8 buffer, scalar scale)`.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

const FP32_MAGIC: u32 = 0x4749_4C32;
const INT8_MAGIC: u32 = 0x4749_4C38;
const FORMAT_VERSION: u32 = 1;

fn main() -> std::io::Result<()> {
    let fp32_path = Path::new("models/gliner2-fp32.bin");
    let int8_path = Path::new("models/gliner2-int8.bin");

    let bytes = std::fs::read(fp32_path)?;
    let mut r = Reader::new(&bytes);

    // ── Header ────────────────────────────────────────────────────
    let magic = r.u32();
    assert_eq!(magic, FP32_MAGIC, "expected fp32 magic at start");
    let format_version = r.u32();
    assert_eq!(format_version, FORMAT_VERSION);
    let num_layers = r.u32();
    let hidden = r.u32();
    let num_heads = r.u32();
    let intermediate = r.u32();
    let vocab = r.u32();
    let max_pos = r.u32();
    let pos_buckets = r.u32();
    let proj_out = r.u32();
    let max_width = r.u32();
    let class_token_index = r.u32();
    let num_lstm_layers = r.u32();
    let layer_norm_eps = r.f32();

    let h = hidden as usize;
    let inter = intermediate as usize;
    let v = vocab as usize;
    let po = proj_out as usize;
    let lstm_half = po / 2;
    let four_h_half = 4 * lstm_half;
    let nl = num_layers as usize;
    let pb = pos_buckets as usize;

    let mut out = BufWriter::new(File::create(int8_path)?);
    write_u32(&mut out, INT8_MAGIC)?;
    write_u32(&mut out, FORMAT_VERSION)?;
    write_u32(&mut out, num_layers)?;
    write_u32(&mut out, hidden)?;
    write_u32(&mut out, num_heads)?;
    write_u32(&mut out, intermediate)?;
    write_u32(&mut out, vocab)?;
    write_u32(&mut out, max_pos)?;
    write_u32(&mut out, pos_buckets)?;
    write_u32(&mut out, proj_out)?;
    write_u32(&mut out, max_width)?;
    write_u32(&mut out, class_token_index)?;
    write_u32(&mut out, num_lstm_layers)?;
    write_f32(&mut out, layer_norm_eps)?;

    // ── Embeddings ────────────────────────────────────────────────
    let word_emb = r.f32_vec(v * h);
    let emb_ln_gamma = r.f32_vec(h);
    let emb_ln_beta = r.f32_vec(h);

    write_quantized_per_tensor(&mut out, &word_emb)?;
    write_f32_slice(&mut out, &emb_ln_gamma)?;
    write_f32_slice(&mut out, &emb_ln_beta)?;

    // ── Per encoder layer ────────────────────────────────────────
    for layer in 0..nl {
        let q_w = r.f32_vec(h * h);
        let q_b = r.f32_vec(h);
        let k_w = r.f32_vec(h * h);
        let k_b = r.f32_vec(h);
        let v_w_layer = r.f32_vec(h * h);
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

        write_quantized_per_channel(&mut out, &q_w, h, h)?;
        write_f32_slice(&mut out, &q_b)?;
        write_quantized_per_channel(&mut out, &k_w, h, h)?;
        write_f32_slice(&mut out, &k_b)?;
        write_quantized_per_channel(&mut out, &v_w_layer, h, h)?;
        write_f32_slice(&mut out, &v_b)?;
        write_quantized_per_channel(&mut out, &attn_out_w, h, h)?;
        write_f32_slice(&mut out, &attn_out_b)?;
        write_f32_slice(&mut out, &attn_ln_g)?;
        write_f32_slice(&mut out, &attn_ln_b)?;
        write_quantized_per_channel(&mut out, &ffn_int_w, h, inter)?;
        write_f32_slice(&mut out, &ffn_int_b)?;
        write_quantized_per_channel(&mut out, &ffn_out_w, inter, h)?;
        write_f32_slice(&mut out, &ffn_out_b)?;
        write_f32_slice(&mut out, &ffn_ln_g)?;
        write_f32_slice(&mut out, &ffn_ln_b)?;

        eprintln!("layer {layer} quantized");
    }

    // ── Encoder shared ───────────────────────────────────────────
    let rel_emb = r.f32_vec(2 * pb * h);
    let rel_ln_g = r.f32_vec(h);
    let rel_ln_b = r.f32_vec(h);
    let final_ln_g = r.f32_vec(h);
    let final_ln_b = r.f32_vec(h);

    write_quantized_per_tensor(&mut out, &rel_emb)?;
    write_f32_slice(&mut out, &rel_ln_g)?;
    write_f32_slice(&mut out, &rel_ln_b)?;
    write_f32_slice(&mut out, &final_ln_g)?;
    write_f32_slice(&mut out, &final_ln_b)?;

    // ── 768 → 512 projection ─────────────────────────────────────
    let proj_w = r.f32_vec(h * po);
    let proj_b = r.f32_vec(po);
    write_quantized_per_channel(&mut out, &proj_w, h, po)?;
    write_f32_slice(&mut out, &proj_b)?;

    // ── BiLSTM — kept fp32 (small footprint, sequential per timestep).
    for _ in 0..2 {
        let ih_w = r.f32_vec(po * four_h_half);
        let hh_w = r.f32_vec(lstm_half * four_h_half);
        let ih_b = r.f32_vec(four_h_half);
        let hh_b = r.f32_vec(four_h_half);
        write_f32_slice(&mut out, &ih_w)?;
        write_f32_slice(&mut out, &hh_w)?;
        write_f32_slice(&mut out, &ih_b)?;
        write_f32_slice(&mut out, &hh_b)?;
    }

    // ── Span head MLPs — quantize each Linear (per-channel) ──────
    // project_start: in=po, inner=4*po, out=po
    // project_end:   same shape
    // out_project:   in=2*po, inner=4*po, out=po
    // prompt:        in=po, inner=4*po, out=po
    let mut quantize_mlp = |out: &mut BufWriter<File>,
                            r: &mut Reader,
                            in_dim: usize|
     -> std::io::Result<()> {
        let inner = 4 * po;
        let lin1_w = r.f32_vec(in_dim * inner);
        let lin1_b = r.f32_vec(inner);
        let lin2_w = r.f32_vec(inner * po);
        let lin2_b = r.f32_vec(po);
        write_quantized_per_channel(out, &lin1_w, in_dim, inner)?;
        write_f32_slice(out, &lin1_b)?;
        write_quantized_per_channel(out, &lin2_w, inner, po)?;
        write_f32_slice(out, &lin2_b)?;
        Ok(())
    };

    quantize_mlp(&mut out, &mut r, po)?; // project_start
    quantize_mlp(&mut out, &mut r, po)?; // project_end
    quantize_mlp(&mut out, &mut r, 2 * po)?; // out_project
    quantize_mlp(&mut out, &mut r, po)?; // prompt

    assert_eq!(
        r.pos,
        bytes.len(),
        "trailing {} bytes in fp32 input",
        bytes.len() - r.pos
    );

    out.flush()?;
    let n = std::fs::metadata(int8_path)?.len();
    println!(
        "wrote {} ({:.1} MB)",
        int8_path.display(),
        n as f64 / 1_048_576.0
    );

    Ok(())
}

// ── Quantization helpers ─────────────────────────────────────────────

fn write_quantized_per_tensor<W: Write>(w: &mut W, t: &[f32]) -> std::io::Result<()> {
    let max_abs = t.iter().copied().map(f32::abs).fold(0.0f32, f32::max);
    let scale = (max_abs / 127.0).max(1e-12);
    let q: Vec<i8> = t.iter().map(|&v| quantize_one(v, scale)).collect();
    write_i8_slice(w, &q)?;
    write_f32(w, scale)
}

fn write_quantized_per_channel<W: Write>(
    w: &mut W,
    t: &[f32],
    in_dim: usize,
    out_dim: usize,
) -> std::io::Result<()> {
    assert_eq!(t.len(), in_dim * out_dim);
    let mut scales = vec![0.0f32; out_dim];
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
    let bytes: &[u8] =
        unsafe { std::slice::from_raw_parts(slice.as_ptr() as *const u8, slice.len() * 4) };
    w.write_all(bytes)
}
fn write_i8_slice<W: Write>(w: &mut W, slice: &[i8]) -> std::io::Result<()> {
    let bytes: &[u8] =
        unsafe { std::slice::from_raw_parts(slice.as_ptr() as *const u8, slice.len()) };
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
        let dst =
            unsafe { std::slice::from_raw_parts_mut(out.as_mut_ptr() as *mut u8, byte_len) };
        dst.copy_from_slice(src);
        self.pos += byte_len;
        out
    }
}
