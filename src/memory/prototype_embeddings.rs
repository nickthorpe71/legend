//! Seed embeddings for amygdala emotional prototypes.
//!
//! Stored as a little-endian f32 blob (100 × 384 = 153,600 bytes) and
//! reinterpreted as `&[[f32; 384]; 100]` at compile time via a 4-byte
//! aligned wrapper. Zero-copy at runtime — the slice points directly into
//! the binary's read-only data.
//!
//! Regenerate with: `cargo run --release --example gen_prototype_embeddings`
//! Must stay in sync with `SEED_PROTOTYPES` (amygdala.rs) — same order and length.

const NUM_PROTOTYPES: usize = 100;
const EMBEDDING_DIM: usize = 384;
const BLOB_LEN: usize = NUM_PROTOTYPES * EMBEDDING_DIM * 4;

#[repr(align(4))]
struct Aligned<const N: usize>([u8; N]);

static BLOB: &Aligned<BLOB_LEN> = &Aligned(*include_bytes!("seed_embeddings.bin"));

// SAFETY: BLOB is 4-byte aligned (via repr(align(4))), exactly BLOB_LEN bytes,
// and any f32 bit pattern is valid. Embeddings are stored little-endian —
// correct on all targets the legend binary currently builds for.
pub static SEED_EMBEDDINGS: &[[f32; EMBEDDING_DIM]; NUM_PROTOTYPES] =
    unsafe { &*(BLOB.0.as_ptr() as *const [[f32; EMBEDDING_DIM]; NUM_PROTOTYPES]) };
