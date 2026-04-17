//! Regenerate `src/memory/seed_embeddings.bin` from `SEED_PROTOTYPES` text.
//!
//! Run: `cargo run --release --example gen_prototype_embeddings`
//!
//! Output format: 100 × 384 little-endian f32, concatenated.
//!
//! CAVEAT: the committed blob was extracted from the pre-v0.4.0 fastembed/ort
//! output. This tool re-embeds via the current tract-onnx runtime, which
//! produces numerically different embeddings for the same input. Running it
//! overwrites the blob and will shift amygdala valence scores enough to break
//! at least `test_prototype_neutral_vs_emotional` until tests are recalibrated.
//! Use deliberately, not casually.

use legend::memory::amygdala::SEED_PROTOTYPES;
use legend::memory::entorhinal::embed_text;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    const DIM: usize = 384;
    let out_path = Path::new("src/memory/seed_embeddings.bin");
    let mut out = BufWriter::new(File::create(out_path)?);

    for (text, _, _) in SEED_PROTOTYPES {
        let emb = embed_text(text, DIM);
        assert_eq!(emb.len(), DIM, "unexpected embedding dim for {text:?}");
        for v in emb {
            out.write_all(&v.to_le_bytes())?;
        }
    }
    out.flush()?;

    println!(
        "Wrote {} prototype embeddings × {} dims to {}",
        SEED_PROTOTYPES.len(),
        DIM,
        out_path.display()
    );
    Ok(())
}
