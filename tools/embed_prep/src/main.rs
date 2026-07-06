// One-time OFFLINE prep for the pure-C MiniLM embedder. Not shipped.
// Emits fp32 weights (embed.c contract), id-ordered vocab, and golden vectors.
use anyhow::{bail, Context, Result};
use candle_core::{Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config, DTYPE};
use safetensors::SafeTensors;
use std::io::Write;
use tokenizers::Tokenizer;

const HIDDEN: usize = 384;
const LAYERS: usize = 6;
const HEADS: usize = 12;
const INTER: usize = 1536;
const VOCAB: usize = 30522;
const MAXPOS: usize = 512;

// Weight dump order == the read order embed.c expects. All little-endian f32.
fn weight_names() -> Vec<String> {
    let mut n = vec![
        "embeddings.word_embeddings.weight".to_string(),
        "embeddings.position_embeddings.weight".to_string(),
        "embeddings.token_type_embeddings.weight".to_string(),
        "embeddings.LayerNorm.weight".to_string(),
        "embeddings.LayerNorm.bias".to_string(),
    ];
    for i in 0..LAYERS {
        let p = format!("encoder.layer.{i}");
        for s in [
            "attention.self.query.weight", "attention.self.query.bias",
            "attention.self.key.weight", "attention.self.key.bias",
            "attention.self.value.weight", "attention.self.value.bias",
            "attention.output.dense.weight", "attention.output.dense.bias",
            "attention.output.LayerNorm.weight", "attention.output.LayerNorm.bias",
            "intermediate.dense.weight", "intermediate.dense.bias",
            "output.dense.weight", "output.dense.bias",
            "output.LayerNorm.weight", "output.LayerNorm.bias",
        ] {
            n.push(format!("{p}.{s}"));
        }
    }
    n
}

fn manifest(rel: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn main() -> Result<()> {
    let device = Device::Cpu;
    // Local assets (config/tokenizer identical for fp32 and quantized); safetensors path via argv.
    let base = manifest("../../models/all-MiniLM-L6-v2-q");
    let cfg_f = base.join("config.json");
    let tok_f = base.join("tokenizer.json");
    let w_f = std::path::PathBuf::from(
        std::env::args().nth(1).context("usage: embed_prep <model.safetensors>")?);
    println!("using: {cfg_f:?}\n       {tok_f:?}\n       {w_f:?}");

    let config: Config = serde_json::from_str(&std::fs::read_to_string(&cfg_f)?)?;
    let mut tokenizer = Tokenizer::from_file(&tok_f).map_err(anyhow::Error::msg)?;
    // No padding/truncation: encode returns the real-length sequence so the mean
    // pool and the golden ids match embed.c (which pads nothing).
    tokenizer.with_padding(None);
    tokenizer.with_truncation(None).map_err(anyhow::Error::msg)?;

    // ---- 1. weight blob (from the fp32 safetensors, in embed.c's order) ----
    let raw = std::fs::read(&w_f)?;
    let st = SafeTensors::deserialize(&raw)?;
    let out_bin = manifest("../../models/all-MiniLM-L6-v2-q/minilm.int8.bin");
    let mut bin = std::io::BufWriter::new(std::fs::File::create(&out_bin)?);
    bin.write_all(b"MINILM02")?;
    for v in [HIDDEN, LAYERS, HEADS, INTER, VOCAB, MAXPOS] {
        bin.write_all(&(v as u32).to_le_bytes())?;
    }
    // Per tensor: u8 quant flag; quant=1 -> f32 scale + i8 data (symmetric per
    // tensor), quant=0 -> raw f32. Only large matrices are quantized; small,
    // sensitive params (biases, LayerNorm, tiny embeddings) stay fp32.
    const QUANT_MIN: usize = 4096;
    let (mut qb, mut fb) = (0usize, 0usize);
    for name in weight_names() {
        let t = st.tensor(&name).with_context(|| format!("missing tensor {name}"))?;
        if t.dtype() != safetensors::Dtype::F32 {
            bail!("{name}: expected F32, got {:?}", t.dtype());
        }
        let raw = t.data();
        let f: Vec<f32> = raw.chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
        if f.len() >= QUANT_MIN {
            // per-row (first-dim) symmetric int8: one scale per output channel /
            // token row -> far better fidelity than a single per-tensor scale.
            let rows = t.shape()[0];
            let cols = f.len() / rows;
            bin.write_all(&[1u8])?;
            let mut q = vec![0u8; f.len()];
            for r in 0..rows {
                let row = &f[r * cols..(r + 1) * cols];
                let amax = row.iter().fold(0f32, |m, &x| m.max(x.abs()));
                let scale = if amax > 0.0 { amax / 127.0 } else { 1.0 };
                bin.write_all(&scale.to_le_bytes())?;
                for c in 0..cols {
                    q[r * cols + c] = ((row[c] / scale).round().clamp(-127.0, 127.0) as i8) as u8;
                }
            }
            bin.write_all(&q)?;
            qb += q.len();
        } else {
            bin.write_all(&[0u8])?;
            bin.write_all(raw)?;
            fb += raw.len();
        }
    }
    bin.flush()?;
    println!("int8 bytes: {qb}, fp32 bytes: {fb}");
    println!("wrote {out_bin:?}");

    // ---- 2. id-ordered vocab.txt ----
    let vocab = tokenizer.get_vocab(true); // token -> id
    let mut by_id = vec![String::new(); VOCAB];
    for (tok, id) in vocab {
        if (id as usize) < VOCAB {
            by_id[id as usize] = tok;
        }
    }
    let out_vocab = manifest("../../models/all-MiniLM-L6-v2-q/vocab.txt");
    let mut vf = std::io::BufWriter::new(std::fs::File::create(&out_vocab)?);
    for tok in &by_id {
        writeln!(vf, "{tok}")?;
    }
    vf.flush()?;
    println!("wrote {out_vocab:?} ({VOCAB} tokens)");

    // ---- 3. golden: sample string -> token ids + mean-pooled L2-normed vector ----
    let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[w_f.clone()], DTYPE, &device)? };
    let model = BertModel::load(vb, &config)?;
    let samples = [
        "the grid", "the battlefield layout", "mana economy", "the caster's fuel supply",
        "PCG32 rng", "the random number source", "healing at the inn", "tavern rest",
    ];
    let out_golden = manifest("golden.txt");
    let mut gf = std::io::BufWriter::new(std::fs::File::create(&out_golden)?);
    for s in samples {
        let enc = tokenizer.encode(s, true).map_err(anyhow::Error::msg)?;
        let ids: Vec<u32> = enc.get_ids().to_vec();
        let n = ids.len();
        let input = Tensor::new(ids.as_slice(), &device)?.unsqueeze(0)?;
        let ttype = input.zeros_like()?;
        let mask = Tensor::ones((1, n), DTYPE, &device)?;
        let hidden = model.forward(&input, &ttype, Some(&mask))?; // [1, n, HIDDEN]
        let mean = (hidden.sum(1)? / n as f64)?; // mask is all ones -> plain mean
        let mean = mean.squeeze(0)?; // [HIDDEN]
        let norm = mean.sqr()?.sum_all()?.sqrt()?.to_scalar::<f32>()?;
        let vec: Vec<f32> = (mean / norm as f64)?.to_vec1()?;
        let idstr = ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
        let vstr = vec.iter().map(|x| format!("{x:.6}")).collect::<Vec<_>>().join(",");
        writeln!(gf, "{s}\t{idstr}\t{vstr}")?;
        println!("golden: {s:?} -> {n} tokens");
    }
    gf.flush()?;
    println!("wrote {out_golden:?}");
    Ok(())
}
