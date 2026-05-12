//! One-shot build tool: read the bundled INT8-quantized ONNX model,
//! extract every weight tensor, dequantize the U8/I8 weights to fp32,
//! and write everything as a packed little-endian binary at
//! `models/minilm-fp32.bin`.
//!
//! Run: `cargo run --release --example extract_weights`
//!
//! Why this exists: tract-onnx (current runtime) is ~100× slower than
//! optimized inference, but its ONNX parser is fine for build-time
//! one-shot use. We use it here to extract weights, then implement our
//! own pure-Rust forward pass against the extracted fp32 weights.
//! After this lands and validates, tract-onnx leaves runtime
//! dependencies entirely.
//!
//! Format (`models/minilm-fp32.bin`, all little-endian):
//!
//! ── Header (10 × u32 + 1 × f32) ──
//!   magic                u32  0x4D4C4D31 ("MLM1")
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
//! ── Embeddings ──
//!   word_emb             [vocab_size, hidden_size] f32
//!   pos_emb              [max_position, hidden_size] f32
//!   type_emb             [type_vocab_size, hidden_size] f32
//!   emb_ln_gamma         [hidden_size] f32
//!   emb_ln_beta          [hidden_size] f32
//!
//! ── Per encoder layer (6×) ──
//!   q_weight             [hidden_size, hidden_size] f32
//!   q_bias               [hidden_size] f32
//!   k_weight             [hidden_size, hidden_size] f32
//!   k_bias               [hidden_size] f32
//!   v_weight             [hidden_size, hidden_size] f32
//!   v_bias               [hidden_size] f32
//!   attn_out_weight      [hidden_size, hidden_size] f32
//!   attn_out_bias        [hidden_size] f32
//!   attn_ln_gamma        [hidden_size] f32
//!   attn_ln_beta         [hidden_size] f32
//!   ffn_int_weight       [hidden_size, intermediate_size] f32
//!   ffn_int_bias         [intermediate_size] f32
//!   ffn_out_weight       [intermediate_size, hidden_size] f32
//!   ffn_out_bias         [hidden_size] f32
//!   ffn_ln_gamma         [hidden_size] f32
//!   ffn_ln_beta          [hidden_size] f32

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use tract_onnx::prelude::*;
use tract_onnx::tract_hir::infer::Factoid;

const MAGIC: u32 = 0x4D4C4D31;
const FORMAT_VERSION: u32 = 1;
const NUM_LAYERS: u32 = 6;
const HIDDEN_SIZE: u32 = 384;
const NUM_HEADS: u32 = 12;
const INTERMEDIATE_SIZE: u32 = 1536;
const VOCAB_SIZE: u32 = 30522;
const MAX_POSITION: u32 = 512;
const TYPE_VOCAB_SIZE: u32 = 2;
const LAYER_NORM_EPS: f32 = 1e-12;

// MatMul ID per (layer, role). Discovered by graph trace (see
// `inspect_onnx`). Order within layer: Q, K, V, AttnOut, FfnInt, FfnOut.
const MATMUL_IDS: [[u32; 6]; 6] = [
    [787, 788, 791, 797, 798, 799],
    [800, 801, 804, 810, 811, 812],
    [813, 814, 817, 823, 824, 825],
    [826, 827, 830, 836, 837, 838],
    [839, 840, 843, 849, 850, 851],
    [852, 853, 856, 862, 863, 864],
];

fn main() -> TractResult<()> {
    let onnx_bytes: &[u8] = include_bytes!("../models/all-MiniLM-L6-v2-q/model.onnx");
    let model = tract_onnx::onnx().model_for_read(&mut std::io::Cursor::new(onnx_bytes))?;

    // Index constants by name → owned tensor.
    let mut consts: HashMap<String, Tensor> = HashMap::new();
    for n in &model.nodes {
        if n.op.name() != "Const" {
            continue;
        }
        // The Const op carries the tensor as its op_for state. Pull it
        // out via downcast — tract's Const stores `Arc<Tensor>`.
        // tract's Const op stores its tensor in the outlet fact's
        // `value` field (a `GenericFactoid<Arc<Tensor>>`).
        if let Ok(fact) = model.outlet_fact(OutletId::new(n.id, 0))
            && let Some(t) = fact.value.concretize()
        {
            consts.insert(n.name.clone(), t.as_ref().clone());
        }
    }
    eprintln!("indexed {} constants", consts.len());

    let out_path = Path::new("models/minilm-fp32.bin");
    std::fs::create_dir_all(out_path.parent().unwrap())?;
    let mut out = BufWriter::new(File::create(out_path)?);

    // Header (10 × u32 + 1 × f32).
    write_u32(&mut out, MAGIC)?;
    write_u32(&mut out, FORMAT_VERSION)?;
    write_u32(&mut out, NUM_LAYERS)?;
    write_u32(&mut out, HIDDEN_SIZE)?;
    write_u32(&mut out, NUM_HEADS)?;
    write_u32(&mut out, INTERMEDIATE_SIZE)?;
    write_u32(&mut out, VOCAB_SIZE)?;
    write_u32(&mut out, MAX_POSITION)?;
    write_u32(&mut out, TYPE_VOCAB_SIZE)?;
    write_u32(&mut out, 0)?; // padding
    write_f32(&mut out, LAYER_NORM_EPS)?;

    // ── Embeddings ──
    // Word / position / type embeddings: stored as U8 quantized with
    // scalar (per-tensor) scale + zero_point. Dequantize:
    //   fp32 = (u8 - zero_point) * scale
    let word_emb = dequant_per_tensor_u8(
        &consts,
        "embeddings.word_embeddings.weight_quantized",
        "embeddings.word_embeddings.weight_scale",
        "embeddings.word_embeddings.weight_zero_point",
    );
    write_f32_slice(&mut out, &word_emb)?;
    eprintln!(
        "wrote word_emb: {} entries (expected {})",
        word_emb.len(),
        VOCAB_SIZE * HIDDEN_SIZE
    );
    assert_eq!(word_emb.len(), (VOCAB_SIZE * HIDDEN_SIZE) as usize);

    let pos_emb = dequant_per_tensor_u8(
        &consts,
        "embeddings.position_embeddings.weight_quantized",
        "embeddings.position_embeddings.weight_scale",
        "embeddings.position_embeddings.weight_zero_point",
    );
    write_f32_slice(&mut out, &pos_emb)?;
    assert_eq!(pos_emb.len(), (MAX_POSITION * HIDDEN_SIZE) as usize);

    let type_emb = dequant_per_tensor_u8(
        &consts,
        "embeddings.token_type_embeddings.weight_quantized",
        "embeddings.token_type_embeddings.weight_scale",
        "embeddings.token_type_embeddings.weight_zero_point",
    );
    write_f32_slice(&mut out, &type_emb)?;
    assert_eq!(type_emb.len(), (TYPE_VOCAB_SIZE * HIDDEN_SIZE) as usize);

    // Embedding LayerNorm — already fp32.
    let emb_ln_gamma = read_f32(&consts, "embeddings.LayerNorm.weight");
    let emb_ln_beta = read_f32(&consts, "embeddings.LayerNorm.bias");
    assert_eq!(emb_ln_gamma.len(), HIDDEN_SIZE as usize);
    write_f32_slice(&mut out, &emb_ln_gamma)?;
    write_f32_slice(&mut out, &emb_ln_beta)?;

    // ── Per-layer weights ──
    for (layer, &[q, k, v, attn_out, ffn_int, ffn_out]) in MATMUL_IDS.iter().enumerate() {
        let q_w = dequant_per_channel_i8(&consts, q);
        let q_b = read_f32(
            &consts,
            &format!("encoder.layer.{layer}.attention.self.query.bias"),
        );
        let k_w = dequant_per_channel_i8(&consts, k);
        let k_b = read_f32(
            &consts,
            &format!("encoder.layer.{layer}.attention.self.key.bias"),
        );
        let v_w = dequant_per_channel_i8(&consts, v);
        let v_b = read_f32(
            &consts,
            &format!("encoder.layer.{layer}.attention.self.value.bias"),
        );
        let attn_out_w = dequant_per_channel_i8(&consts, attn_out);
        let attn_out_b = read_f32(
            &consts,
            &format!("encoder.layer.{layer}.attention.output.dense.bias"),
        );
        let attn_ln_g = read_f32(
            &consts,
            &format!("encoder.layer.{layer}.attention.output.LayerNorm.weight"),
        );
        let attn_ln_b = read_f32(
            &consts,
            &format!("encoder.layer.{layer}.attention.output.LayerNorm.bias"),
        );
        let ffn_int_w = dequant_per_channel_i8(&consts, ffn_int);
        let ffn_int_b = read_f32(
            &consts,
            &format!("encoder.layer.{layer}.intermediate.dense.bias"),
        );
        let ffn_out_w = dequant_per_channel_i8(&consts, ffn_out);
        let ffn_out_b = read_f32(&consts, &format!("encoder.layer.{layer}.output.dense.bias"));
        let ffn_ln_g = read_f32(
            &consts,
            &format!("encoder.layer.{layer}.output.LayerNorm.weight"),
        );
        let ffn_ln_b = read_f32(
            &consts,
            &format!("encoder.layer.{layer}.output.LayerNorm.bias"),
        );

        // Shape sanity.
        assert_eq!(q_w.len(), (HIDDEN_SIZE * HIDDEN_SIZE) as usize);
        assert_eq!(q_b.len(), HIDDEN_SIZE as usize);
        assert_eq!(attn_out_w.len(), (HIDDEN_SIZE * HIDDEN_SIZE) as usize);
        assert_eq!(ffn_int_w.len(), (HIDDEN_SIZE * INTERMEDIATE_SIZE) as usize);
        assert_eq!(ffn_out_w.len(), (INTERMEDIATE_SIZE * HIDDEN_SIZE) as usize);
        assert_eq!(ffn_int_b.len(), INTERMEDIATE_SIZE as usize);

        for slab in [
            &q_w,
            &q_b,
            &k_w,
            &k_b,
            &v_w,
            &v_b,
            &attn_out_w,
            &attn_out_b,
            &attn_ln_g,
            &attn_ln_b,
            &ffn_int_w,
            &ffn_int_b,
            &ffn_out_w,
            &ffn_out_b,
            &ffn_ln_g,
            &ffn_ln_b,
        ] {
            write_f32_slice(&mut out, slab)?;
        }
        eprintln!("layer {layer} done");
    }

    out.flush()?;
    let bytes = std::fs::metadata(out_path)?.len();
    println!(
        "wrote {} ({:.1} MB)",
        out_path.display(),
        bytes as f64 / 1_048_576.0
    );

    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────

fn write_u32<W: Write>(w: &mut W, v: u32) -> std::io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

fn write_f32<W: Write>(w: &mut W, v: f32) -> std::io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

fn write_f32_slice<W: Write>(w: &mut W, slice: &[f32]) -> std::io::Result<()> {
    // SAFETY: f32 has a stable little-endian representation on x86_64.
    // We rely on the platform being LE (asserted at runtime in
    // src/embed/weights.rs).
    let bytes: &[u8] =
        unsafe { std::slice::from_raw_parts(slice.as_ptr() as *const u8, slice.len() * 4) };
    w.write_all(bytes)
}

fn read_f32(consts: &HashMap<String, Tensor>, name: &str) -> Vec<f32> {
    let t = consts
        .get(name)
        .unwrap_or_else(|| panic!("missing constant {name}"));
    let view = t
        .as_slice::<f32>()
        .unwrap_or_else(|_| panic!("{name} is not f32"));
    view.to_vec()
}

fn read_u8(consts: &HashMap<String, Tensor>, name: &str) -> Vec<u8> {
    let t = consts
        .get(name)
        .unwrap_or_else(|| panic!("missing constant {name}"));
    let view = t
        .as_slice::<u8>()
        .unwrap_or_else(|_| panic!("{name} is not u8"));
    view.to_vec()
}

fn read_i8_opt(consts: &HashMap<String, Tensor>, name: &str) -> Option<Vec<i8>> {
    let t = consts.get(name)?;
    Some(
        t.as_slice::<i8>()
            .unwrap_or_else(|_| panic!("{name} present but not i8"))
            .to_vec(),
    )
}

/// Dequantize a per-tensor U8 quantized embedding. Scale is scalar
/// (single f32), zero_point is scalar (single u8).
fn dequant_per_tensor_u8(
    consts: &HashMap<String, Tensor>,
    q_name: &str,
    scale_name: &str,
    zp_name: &str,
) -> Vec<f32> {
    let q = read_u8(consts, q_name);
    let scale_arr = read_f32(consts, scale_name);
    assert_eq!(scale_arr.len(), 1, "{scale_name} expected scalar");
    let scale = scale_arr[0];
    let zp = consts
        .get(zp_name)
        .map(|t| {
            let v = t
                .as_slice::<u8>()
                .unwrap_or_else(|_| panic!("{zp_name} not u8"));
            assert_eq!(v.len(), 1, "{zp_name} expected scalar");
            v[0] as i32
        })
        .unwrap_or(0);
    q.iter().map(|&u| (u as i32 - zp) as f32 * scale).collect()
}

/// Dequantize a per-output-channel I8 quantized MatMul weight.
/// Shape: (in_features, out_features); scale: (out_features,);
/// optional zero_point: (out_features,) of i8.
///
/// `fp32[i, j] = (i8[i, j] - zp[j]) * scale[j]`
fn dequant_per_channel_i8(consts: &HashMap<String, Tensor>, mm_id: u32) -> Vec<f32> {
    let q_name = format!("onnx::MatMul_{mm_id}_quantized");
    let scale_name = format!("onnx::MatMul_{mm_id}_scale");
    let zp_name = format!("onnx::MatMul_{mm_id}_zero_point");

    let q_t = consts
        .get(&q_name)
        .unwrap_or_else(|| panic!("missing {q_name}"));
    let q = q_t
        .as_slice::<i8>()
        .unwrap_or_else(|_| panic!("{q_name} not i8"));
    let q_shape: Vec<usize> = q_t.shape().to_vec();
    assert_eq!(q_shape.len(), 2, "{q_name} expected 2D");
    let in_dim = q_shape[0];
    let out_dim = q_shape[1];

    let scale = read_f32(consts, &scale_name);
    assert_eq!(scale.len(), out_dim, "{scale_name} expected {out_dim}");

    let zp = read_i8_opt(consts, &zp_name).unwrap_or_else(|| vec![0i8; out_dim]);
    assert_eq!(zp.len(), out_dim, "{zp_name} expected {out_dim}");

    let mut result = Vec::with_capacity(in_dim * out_dim);
    for i in 0..in_dim {
        for j in 0..out_dim {
            let v = q[i * out_dim + j] as i32 - zp[j] as i32;
            result.push(v as f32 * scale[j]);
        }
    }
    result
}
