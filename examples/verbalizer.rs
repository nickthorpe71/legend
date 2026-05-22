//! Shared verbalizer: loads SmolLM2-360M-Instruct via candle and
//! answers questions grounded in a provided context. Used by both
//! `verbalize_smoke.rs` (single-prompt sanity check) and
//! `bench_memoryagentbench_fc.rs` (per-question scoring against
//! gold answers).
//!
//! Imported via `#[path]` because candle is a dev-dep — it can't
//! live in the main crate without dragging the model framework into
//! the runtime binary. See `examples/shared/mod.rs` for the same
//! pattern with `serde`.
//!
//! ```ignore
//! #[path = "verbalizer.rs"]
//! mod verbalizer;
//! use verbalizer::Verbalizer;
//! ```
//!
//! Weights are fetched via `curl` rather than a Rust HF client
//! because HF's resolve endpoint returns 307s with relative
//! `location:` headers, which hf-hub fumbles on the current backend.

#![allow(dead_code)] // shared between two example binaries; each
                     // uses a subset of the surface.

use anyhow::{Context, Result, anyhow};
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::llama::{Cache, Config, Llama, LlamaConfig};
use std::path::PathBuf;
use std::process::Command;
use tokenizers::Tokenizer;

const REPO: &str = "HuggingFaceTB/SmolLM2-360M-Instruct";
const CACHE_ROOT: &str = "benchmarks/models";

/// Loads the model + tokenizer once; can answer many prompts. The
/// KV cache is recreated per prompt — keeping one cache across
/// independent prompts would poison the state.
pub struct Verbalizer {
    tokenizer: Tokenizer,
    model: Llama,
    config: Config,
    device: Device,
    dtype: DType,
    eos_id: u32,
    pub max_new_tokens: usize,
}

impl Verbalizer {
    /// Load (or fetch) weights + tokenizer, then build the model on
    /// CPU in F16. F16 halves memory bandwidth vs F32 and lets AVX2
    /// chew through the prompt-prefill phase materially faster on
    /// CPUs without dedicated BF16 support. Quality loss for short-
    /// answer extraction at this model size is negligible.
    pub fn load() -> Result<Self> {
        let config_path = fetch_file("config.json")?;
        let tokenizer_path = fetch_file("tokenizer.json")?;
        let weights_path = fetch_file("model.safetensors")?;

        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| anyhow!("load tokenizer: {e}"))?;
        let eos_id = tokenizer
            .token_to_id("<|im_end|>")
            .context("missing <|im_end|> in tokenizer")?;

        let device = Device::Cpu;
        let dtype = DType::F16;

        let llama_config_json: LlamaConfig =
            serde_json::from_slice(&std::fs::read(&config_path)?).context("parse config.json")?;
        let config: Config = llama_config_json.into_config(false /* flash_attn */);

        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[weights_path], dtype, &device)
                .context("mmap safetensors")?
        };
        let model = Llama::load(vb, &config).context("build Llama model")?;

        Ok(Self {
            tokenizer,
            model,
            config,
            device,
            dtype,
            eos_id,
            max_new_tokens: 64,
        })
    }

    /// Given a question and a context (Legend's flat frame, or any
    /// other text), generate a short answer.  Uses a ChatML prompt
    /// constraining the model to extract from the provided context
    /// rather than rely on parametric memory.
    pub fn verbalize(&mut self, question: &str, context: &str) -> Result<String> {
        // Tight extraction prompt — keeps output short and grounded.
        let prompt = format!(
            "<|im_start|>system\n\
             You are a fact extractor. Answer using only the provided context. \
             Reply with just the answer in 1-5 words, no explanation.<|im_end|>\n\
             <|im_start|>user\n\
             Context:\n{context}\n\n\
             Question: {question}\n\
             Answer:<|im_end|>\n\
             <|im_start|>assistant\n"
        );

        let encoding = self
            .tokenizer
            .encode(prompt.as_str(), true)
            .map_err(|e| anyhow!("tokenize prompt: {e}"))?;
        let prompt_tokens = encoding.get_ids().to_vec();
        let prompt_len = prompt_tokens.len();

        // Fresh KV cache per prompt — independent prompts must not
        // share cache state.
        let mut cache = Cache::new(true, self.dtype, &self.config, &self.device)
            .context("build kv cache")?;

        let mut all_tokens = prompt_tokens;
        let mut index_pos = 0usize;
        for _ in 0..self.max_new_tokens {
            let context_size = if index_pos == 0 { all_tokens.len() } else { 1 };
            let start = all_tokens.len() - context_size;
            let slice = &all_tokens[start..];

            let input = Tensor::new(slice, &self.device)?.unsqueeze(0)?;
            let logits = self.model.forward(&input, index_pos, &mut cache)?;
            // candle's Llama.forward already slices to the last
            // sequence position internally, so logits is [1, vocab].
            let logits = logits.squeeze(0)?;
            let next_token = logits.argmax(0)?.to_scalar::<u32>()?;

            all_tokens.push(next_token);
            index_pos += context_size;
            if next_token == self.eos_id {
                break;
            }
        }

        let new_tokens = &all_tokens[prompt_len..];
        let answer = self
            .tokenizer
            .decode(new_tokens, true)
            .map_err(|e| anyhow!("decode: {e}"))?;
        Ok(answer)
    }
}

// ─── File fetching ──────────────────────────────────────────────────

fn model_dir() -> PathBuf {
    PathBuf::from(CACHE_ROOT).join(REPO.split('/').next_back().unwrap_or(REPO))
}

/// Download a single file from HF if it isn't already cached. Uses
/// curl so HF's relative-URL redirects get resolved correctly (the
/// hf-hub crate fumbles those on the current HF backend).
pub fn fetch_file(filename: &str) -> Result<PathBuf> {
    let dest = model_dir().join(filename);
    if dest.exists() {
        return Ok(dest);
    }
    std::fs::create_dir_all(
        dest.parent()
            .ok_or_else(|| anyhow!("dest has no parent: {}", dest.display()))?,
    )?;
    let url = format!("https://huggingface.co/{REPO}/resolve/main/{filename}");
    eprintln!("fetching {filename} → {}", dest.display());
    let status = Command::new("curl")
        .args([
            "-L",
            "--fail",
            "--retry",
            "3",
            "--progress-bar",
            "-o",
            dest.to_str()
                .ok_or_else(|| anyhow!("non-utf8 cache path"))?,
            &url,
        ])
        .status()
        .context("spawn curl")?;
    if !status.success() {
        let _ = std::fs::remove_file(&dest);
        return Err(anyhow!(
            "curl exit {:?} fetching {}",
            status.code(),
            filename
        ));
    }
    Ok(dest)
}
