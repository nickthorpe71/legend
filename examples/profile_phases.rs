//! Granular phase profiling of the INT8 forward pass. Isolates each
//! component so we can target the dominant cost.

use std::time::Instant;

use legend::embed::embed_text;
use legend::inference::ops::{
    gelu_inplace, l2_normalize_inplace, layernorm_inplace, masked_mean_pool, matmul, matmul_at_bt,
    softmax_inplace,
};
use legend::inference::quantized_ops::{
    QMatmulScratch, quantize_activation, quantized_matmul_prequant,
};
use legend::inference::{WeightsInt8, bert_int8};
use tokenizers::Tokenizer;

fn time<F: FnMut() -> R, R>(label: &str, iters: usize, mut f: F) -> f64 {
    let _ = f();
    let _ = f();
    let t = Instant::now();
    for _ in 0..iters {
        let _ = f();
    }
    let elapsed = t.elapsed();
    let us_per_iter = elapsed.as_micros() as f64 / iters as f64;
    println!("  {label:<50} {us_per_iter:>8.2} μs");
    us_per_iter
}

fn main() {
    let _ = embed_text("warm up");

    let phrase = "I prefer green tea to black coffee on a Sunday afternoon";

    // 1. End-to-end calls.
    let iters = 100;
    let t = Instant::now();
    for _ in 0..iters {
        let _ = embed_text(phrase);
    }
    let total = t.elapsed();
    let total_us = total.as_micros() as f64 / iters as f64;
    println!("Total per embed_text: {total_us:.2} μs");
    println!();

    // 2. Isolate components.
    let tokenizer_bytes: &[u8] = include_bytes!("../models/all-MiniLM-L6-v2-q/tokenizer.json");
    let mut tok = Tokenizer::from_bytes(tokenizer_bytes).expect("load tokenizer");
    let _ = tok.with_truncation(Some(tokenizers::TruncationParams {
        max_length: 512,
        ..Default::default()
    }));
    tok.with_padding(None); // match the real embed_text path

    let enc = tok.encode(phrase, true).expect("tokenize");
    let ids: Vec<u32> = enc.get_ids().to_vec();
    let mask: Vec<u32> = enc.get_attention_mask().to_vec();
    let seq_len = ids.len();
    println!("Input: \"{phrase}\" → {seq_len} tokens");
    println!();

    let w = WeightsInt8::load_bundled();
    let h = w.hidden_size;
    let inter = w.intermediate_size;
    let head_dim = w.head_dim;
    let num_heads = w.num_heads;

    println!("Per-call component costs (averaged over 1000 iters):");

    let tok_time = time("tokenize", 1000, || {
        tok.encode(phrase, true).expect("tokenize")
    });

    let forward_time = time("bert_int8::forward (no tokenize)", 100, || {
        bert_int8::forward(w, &ids, &mask)
    });

    // Build a representative activation tensor for sub-component timing.
    let activation: Vec<f32> = (0..seq_len * h)
        .map(|i| ((i as f32 * 0.013).sin() - 0.3).clamp(-1.0, 1.0))
        .collect();
    let mut scratch = QMatmulScratch::default();
    quantize_activation(&activation, seq_len, h, &mut scratch);
    let layer = &w.layers[0];

    let mut out_h = vec![0.0f32; seq_len * h];
    let mut out_inter = vec![0.0f32; seq_len * inter];

    let qkv_time = time("matmul QKV (h→h)", 5000, || {
        quantized_matmul_prequant(
            seq_len,
            h,
            h,
            &layer.q_w,
            layer.q_b,
            &mut out_h,
            &mut scratch,
        );
    });
    let ffn_int_time = time("matmul FFN-int (h→inter)", 5000, || {
        quantized_matmul_prequant(
            seq_len,
            h,
            inter,
            &layer.ffn_int_w,
            layer.ffn_int_b,
            &mut out_inter,
            &mut scratch,
        );
    });
    let ffn_out_time = time("matmul FFN-out (inter→h)", 5000, || {
        quantized_matmul_prequant(
            seq_len,
            inter,
            h,
            &layer.ffn_out_w,
            layer.ffn_out_b,
            &mut out_h,
            &mut scratch,
        );
    });

    // Per-row activation quantize.
    let aq_time = time("activation quantize (seq×h)", 5000, || {
        quantize_activation(&activation, seq_len, h, &mut scratch);
    });

    // LayerNorm
    let mut x_ln = activation.clone();
    let ln_g = &layer.attn_ln_gamma;
    let ln_b = &layer.attn_ln_beta;
    let ln_time = time("layernorm (seq×h)", 5000, || {
        layernorm_inplace(&mut x_ln, seq_len, h, ln_g, ln_b, 1e-12);
    });

    // GELU on intermediate-sized tensor.
    let mut ffn_int_buf = vec![0.1f32; seq_len * inter];
    let gelu_time = time("gelu (seq×inter)", 5000, || {
        gelu_inplace(&mut ffn_int_buf);
        // restore so each iter has same input
        for v in ffn_int_buf.iter_mut() {
            *v = 0.1;
        }
    });

    // Softmax over [seq, seq] (per head per layer)
    let mut scores = vec![0.5f32; seq_len * seq_len];
    let softmax_time = time("softmax (seq×seq, per head)", 5000, || {
        softmax_inplace(&mut scores, seq_len, seq_len);
        for v in scores.iter_mut() {
            *v = 0.5;
        }
    });

    // matmul_at_bt — used inside attention
    let q_head = vec![0.5f32; seq_len * head_dim];
    let k_head = vec![0.5f32; seq_len * head_dim];
    let mut scores_buf = vec![0.0f32; seq_len * seq_len];
    let attn_qkt_time = time("matmul Q·K^T (seq×head_dim → seq×seq)", 5000, || {
        matmul_at_bt(
            seq_len,
            head_dim,
            seq_len,
            &q_head,
            &k_head,
            &mut scores_buf,
        );
    });

    let v_head = vec![0.5f32; seq_len * head_dim];
    let mut head_out = vec![0.0f32; seq_len * head_dim];
    let attn_sv_time = time(
        "matmul scores·V (seq×seq → seq×head_dim)",
        5000,
        || {
            matmul(
                seq_len,
                seq_len,
                head_dim,
                &scores_buf,
                &v_head,
                &mut head_out,
            );
        },
    );

    let pool = vec![0.5f32; seq_len * h];
    let mask_f: Vec<f32> = mask.iter().map(|&m| m as f32).collect();
    let pool_time = time("masked_mean_pool + l2", 5000, || {
        let mut p = masked_mean_pool(&pool, seq_len, h, &mask_f);
        l2_normalize_inplace(&mut p);
    });

    println!();
    println!("Estimated per-tick breakdown:");
    let est_matmul = 6.0 * (4.0 * qkv_time + ffn_int_time + ffn_out_time);
    let est_aq = 6.0 * 4.0 * aq_time; // 4 distinct activations per layer
    let est_ln = 13.0 * ln_time; // 1 embed LN + 12 layer LNs
    let est_gelu = 6.0 * gelu_time;
    let est_softmax = 6.0 * num_heads as f64 * softmax_time;
    let est_attn_inner = 6.0 * num_heads as f64 * (attn_qkt_time + attn_sv_time);
    let est_pool = pool_time;
    let est_tok = tok_time;
    println!("  Tokenize                {est_tok:>8.0} μs");
    println!("  Matmul (weight)         {est_matmul:>8.0} μs");
    println!("  Activation quant        {est_aq:>8.0} μs");
    println!("  LayerNorm               {est_ln:>8.0} μs");
    println!("  GELU                    {est_gelu:>8.0} μs");
    println!("  Softmax (per-head)      {est_softmax:>8.0} μs");
    println!("  Attn inner matmul       {est_attn_inner:>8.0} μs");
    println!("  Pool + L2               {est_pool:>8.0} μs");
    let estimated_sum =
        est_tok + est_matmul + est_aq + est_ln + est_gelu + est_softmax + est_attn_inner + est_pool;
    println!("  -------- sum:           {estimated_sum:>8.0} μs");
    println!("  Measured forward+token: {total_us:>8.0} μs");
    println!(
        "  Unaccounted (alloc, copies, etc): {:.0} μs",
        total_us - estimated_sum
    );
    println!();
    println!(
        "Forward-only (no tokenize): {forward_time:.0} μs ({:.1}× faster than total)",
        total_us / forward_time
    );
}
