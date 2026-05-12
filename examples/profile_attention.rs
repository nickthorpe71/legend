//! Instrumented timing of bert_int8::forward subsections by
//! re-running the same code paths externally with `Instant::now()`
//! between them. The phase profiler shows ~455 μs "unaccounted" —
//! this localizes where that hides.

use std::time::Instant;

use legend::embed::embed_text;
use legend::inference::{bert_int8, WeightsInt8};
use legend::inference::ops::{
    add_inplace, gelu_inplace, l2_normalize_inplace, layernorm_inplace, masked_mean_pool,
    softmax_inplace,
};
use legend::inference::quantized_ops::{
    dequant_and_sum_token_embedding, quantize_activation, quantized_matmul_prequant, QMatmulScratch,
};
use tokenizers::Tokenizer;

fn time<F: FnMut()>(label: &str, iters: usize, mut f: F) -> f64 {
    let _ = f();
    let _ = f();
    let t = Instant::now();
    for _ in 0..iters {
        let _ = f();
    }
    let us = t.elapsed().as_micros() as f64 / iters as f64;
    println!("  {label:<55} {us:>8.2} μs");
    us
}

fn main() {
    let _ = embed_text("warm up");

    let tokenizer_bytes: &[u8] = include_bytes!("../models/all-MiniLM-L6-v2-q/tokenizer.json");
    let mut tok = Tokenizer::from_bytes(tokenizer_bytes).expect("load tokenizer");
    let _ = tok.with_truncation(Some(tokenizers::TruncationParams {
        max_length: 512,
        ..Default::default()
    }));
    tok.with_padding(None);

    let phrase = "I prefer green tea to black coffee on a Sunday afternoon";
    let enc = tok.encode(phrase, true).unwrap();
    let ids: Vec<u32> = enc.get_ids().to_vec();
    let mask: Vec<u32> = enc.get_attention_mask().to_vec();
    let seq_len = ids.len();
    println!("seq_len = {seq_len}");

    let w = WeightsInt8::load_bundled();
    let h = w.hidden_size;
    let inter = w.intermediate_size;
    let head_dim = w.head_dim;
    let num_heads = w.num_heads;

    println!("\nSubsection timing per forward (averaged over 500 iters):");

    // ── Embedding lookup
    let mut x_buf = vec![0.0f32; seq_len * h];
    let emb_time = time("embedding lookup loop (13 tokens)", 5000, || {
        for t in 0..seq_len {
            let dst = &mut x_buf[t * h..(t + 1) * h];
            dequant_and_sum_token_embedding(
                &w.word_emb.q_data,
                w.word_emb.scale,
                ids[t] as usize,
                &w.pos_emb.q_data,
                w.pos_emb.scale,
                t,
                &w.type_emb.q_data,
                w.type_emb.scale,
                h,
                dst,
            );
        }
    });

    // ── Embedding LN
    let mut x_ln = x_buf.clone();
    let ln_emb_time = time("embedding LN", 5000, || {
        x_ln.copy_from_slice(&x_buf);
        layernorm_inplace(
            &mut x_ln,
            seq_len,
            h,
            &w.emb_ln_gamma,
            &w.emb_ln_beta,
            w.layer_norm_eps,
        );
    });

    // ── One run_layer_int8
    let mask_f: Vec<f32> = mask.iter().map(|&m| m as f32).collect();
    let mask_bias: Vec<f32> = mask_f
        .iter()
        .map(|&m| if m > 0.0 { 0.0 } else { -1.0e9 })
        .collect();

    // Build representative activation
    let activation: Vec<f32> = (0..seq_len * h)
        .map(|i| ((i as f32 * 0.013).sin() - 0.3).clamp(-1.0, 1.0))
        .collect();
    let layer = &w.layers[0];

    // Per-layer-section timings using primitives.
    let mut q_buf = vec![0.0f32; seq_len * h];
    let mut k_buf = vec![0.0f32; seq_len * h];
    let mut v_buf = vec![0.0f32; seq_len * h];
    let mut scratch = QMatmulScratch::default();

    let qkv_time = time("3× QKV matmul (post-activation-quant)", 1000, || {
        quantize_activation(&activation, seq_len, h, &mut scratch);
        quantized_matmul_prequant(seq_len, h, h, &layer.q_w, &layer.q_b, &mut q_buf, &mut scratch);
        quantized_matmul_prequant(seq_len, h, h, &layer.k_w, &layer.k_b, &mut k_buf, &mut scratch);
        quantized_matmul_prequant(seq_len, h, h, &layer.v_w, &layer.v_b, &mut v_buf, &mut scratch);
    });

    // ── Attention (new strided form)
    let mut scores = vec![0.0f32; seq_len * seq_len];
    let mut attn_concat = vec![0.0f32; seq_len * h];
    let scale = 1.0 / (head_dim as f32).sqrt();
    let attn_time = time("multi-head attention block (12 heads)", 1000, || {
        for cell in attn_concat.iter_mut() {
            *cell = 0.0;
        }
        for hh in 0..num_heads {
            let head_off = hh * head_dim;
            for i in 0..seq_len {
                let q_row = &q_buf[i * h + head_off..i * h + head_off + head_dim];
                let scores_row = &mut scores[i * seq_len..(i + 1) * seq_len];
                for j in 0..seq_len {
                    let k_row = &k_buf[j * h + head_off..j * h + head_off + head_dim];
                    let mut s = 0.0;
                    for d in 0..head_dim {
                        s += q_row[d] * k_row[d];
                    }
                    scores_row[j] = s * scale + mask_bias[j];
                }
            }
            softmax_inplace(&mut scores, seq_len, seq_len);
            for i in 0..seq_len {
                let scores_row = &scores[i * seq_len..(i + 1) * seq_len];
                let out_row =
                    &mut attn_concat[i * h + head_off..i * h + head_off + head_dim];
                for d in 0..head_dim {
                    out_row[d] = 0.0;
                }
                for j in 0..seq_len {
                    let s = scores_row[j];
                    let v_row = &v_buf[j * h + head_off..j * h + head_off + head_dim];
                    for d in 0..head_dim {
                        out_row[d] += s * v_row[d];
                    }
                }
            }
        }
    });

    // ── Attention out projection
    let mut attn_out = vec![0.0f32; seq_len * h];
    let attn_proj_time = time("attn output projection", 5000, || {
        quantized_matmul_prequant(
            seq_len,
            h,
            h,
            &layer.attn_out_w,
            &layer.attn_out_b,
            &mut attn_out,
            &mut scratch,
        );
    });

    // ── add + LN
    let mut x_block = activation.clone();
    let add_ln_time = time("add residual + post-attn LN", 5000, || {
        x_block.copy_from_slice(&activation);
        add_inplace(&mut x_block, &attn_out);
        layernorm_inplace(
            &mut x_block,
            seq_len,
            h,
            &layer.attn_ln_gamma,
            &layer.attn_ln_beta,
            w.layer_norm_eps,
        );
    });

    // ── FFN int + GELU + FFN out + add + LN
    let mut ffn_int = vec![0.0f32; seq_len * inter];
    let mut ffn_out = vec![0.0f32; seq_len * h];
    let ffn_time = time("FFN (quant + gelu + quant + add + LN)", 1000, || {
        quantized_matmul_prequant(
            seq_len,
            h,
            inter,
            &layer.ffn_int_w,
            &layer.ffn_int_b,
            &mut ffn_int,
            &mut scratch,
        );
        gelu_inplace(&mut ffn_int);
        quantized_matmul_prequant(
            seq_len,
            inter,
            h,
            &layer.ffn_out_w,
            &layer.ffn_out_b,
            &mut ffn_out,
            &mut scratch,
        );
        add_inplace(&mut x_block, &ffn_out);
        layernorm_inplace(
            &mut x_block,
            seq_len,
            h,
            &layer.ffn_ln_gamma,
            &layer.ffn_ln_beta,
            w.layer_norm_eps,
        );
    });

    // ── Pool + L2
    let mut pool_buf = vec![0.5f32; seq_len * h];
    let pool_time = time("pool + L2", 5000, || {
        let mut p = masked_mean_pool(&pool_buf, seq_len, h, &mask_f);
        l2_normalize_inplace(&mut p);
        for v in pool_buf.iter_mut() {
            *v *= 1.0;
        }
    });

    println!();
    println!("Estimated forward = emb + LN + 6*(QKV + attn + proj + add_ln + FFN) + pool:");
    let per_layer = qkv_time + attn_time + attn_proj_time + add_ln_time + ffn_time;
    let total = emb_time + ln_emb_time + 6.0 * per_layer + pool_time;
    println!("  emb              {emb_time:>8.0} μs");
    println!("  emb LN           {ln_emb_time:>8.0} μs");
    println!("  per_layer × 6    {per_layer:>8.0} μs × 6 = {:.0} μs", per_layer * 6.0);
    println!("    QKV          {qkv_time:>8.0} μs/layer");
    println!("    attn block   {attn_time:>8.0} μs/layer");
    println!("    attn proj    {attn_proj_time:>8.0} μs/layer");
    println!("    add+LN       {add_ln_time:>8.0} μs/layer");
    println!("    FFN          {ffn_time:>8.0} μs/layer");
    println!("  pool             {pool_time:>8.0} μs");
    println!("  -- estimated total: {total:>8.0} μs");

    // ── Measure actual forward
    let iters = 1000;
    let t = Instant::now();
    for _ in 0..iters {
        let _ = bert_int8::forward(w, &ids, &mask);
    }
    let measured = t.elapsed().as_micros() as f64 / iters as f64;
    println!("  -- measured forward: {measured:>8.0} μs");
    println!("  -- gap (alloc/return/copies): {:>4.0} μs", measured - total);
}
