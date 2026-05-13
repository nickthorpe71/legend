//! Convert `models/gliner2-tokenizer/tokenizer.json` (HuggingFace
//! JSON) to `tokenizer.bin` (bincode) once at build time. The runtime
//! loader (`src/inference/deberta/tokenizer.rs`) then deserializes
//! via bincode, which is much faster than re-parsing the 8 MB JSON
//! on every cold start.
//!
//! Run: `cargo run --release --example serialize_gliner_tokenizer`

use std::fs;
use std::path::Path;

use tokenizers::Tokenizer;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let json_path = Path::new("models/gliner2-tokenizer/tokenizer.json");
    let bin_path = Path::new("models/gliner2-tokenizer/tokenizer.bin");

    eprintln!("loading {}", json_path.display());
    let json_bytes = fs::read(json_path)?;
    let tokenizer = Tokenizer::from_bytes(&json_bytes)?;

    eprintln!("serializing to MessagePack at {}", bin_path.display());
    // `to_vec_named` keeps struct fields named so the deserializer
    // can decode by-name (matches what `from_slice::<Tokenizer>`
    // expects). The default `to_vec` packs structs as positional
    // tuples and breaks at decode time.
    let bin = rmp_serde::to_vec_named(&tokenizer)?;
    fs::write(bin_path, &bin)?;

    let json_mb = json_bytes.len() as f64 / 1_048_576.0;
    let bin_mb = bin.len() as f64 / 1_048_576.0;
    println!("tokenizer.json  {json_mb:.2} MB");
    println!("tokenizer.bin   {bin_mb:.2} MB  ({:.0}%)", bin_mb / json_mb * 100.0);
    Ok(())
}
