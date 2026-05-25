//! Generate a markdown snapshot of the loaded seed Hypergraph.
//!
//! Run: `cargo run --release --example dump_hypergraph_md`
//!
//! Output: `inspect/seed.md` (gitignored).
//!
//! The renderer itself lives at `src/render.rs` so `lib::run` can also
//! emit a fresh snapshot per tick. This example is now a thin shim
//! around `render::render` for the seed-graph use case.

use legend::render::render;
use legend::seed::load_seed_graph;
use std::fs;
use std::io::Write;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let hypergraph = load_seed_graph();
    let out_path = Path::new("inspect/seed.md");
    fs::create_dir_all(out_path.parent().unwrap())?;
    let md = render(&hypergraph);
    let mut file = fs::File::create(out_path)?;
    file.write_all(md.as_bytes())?;
    println!("wrote {} ({} bytes)", out_path.display(), md.len());
    Ok(())
}
