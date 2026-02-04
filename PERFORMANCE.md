# Performance Constraints

Legend is called on **every LLM interaction**, so performance is critical. These constraints ensure Legend stays fast and lightweight.

---

## Hard Performance Targets

### Latency (Per Operation)
- **`legend get_state`**: <5ms (target: <2ms)
- **`legend update`**: <5ms (target: <3ms)
- **`legend show`**: <10ms (human-facing, less critical)

**Why <5ms?**
- Called on every Claude message
- Must feel instant to user
- 5ms is imperceptible latency budget
- Leaves room for disk I/O variance

### Storage Size
- **Compressed on-disk**: <100KB (target: 20-30KB)
- **Uncompressed in-memory**: <200KB (target: 50-100KB)
- **Config file**: <5KB

**Why these limits?**
- Fast to load into memory entirely
- Cheap to compress/decompress
- Reasonable for thousands of AI sessions
- Forces prioritization of essential data

### Scalability Limits
- **Max features**: 1,000 (target: 100-300)
- **Max files tracked**: 5,000 (target: 500-1,000)
- **Max file path length**: 512 bytes
- **Max feature description**: 2KB

**Why these limits?**
- Typical project has 10-50 active features
- Most projects <1,000 source files
- Keeps data structures simple and fast
- Prevents bloat from runaway metadata

---

## Storage Strategy

### Format: Bincode + LZ4 Compression

**Bincode serialization:**
- Rust-native binary format
- ~3-5x smaller than JSON
- Extremely fast: GB/s serialization speed
- Zero parsing overhead

**LZ4 compression:**
- Fastest decompression: >2GB/s
- ~2-3x additional compression
- <1ms overhead for Legend's data size
- Better than Snappy or Zstd for latency

**Combined:**
- JSON: 150KB → Bincode: 50KB → LZ4: 20KB
- **7.5x total compression**
- <2ms to decompress and deserialize

### File Structure
```
.legend/
├── config.toml          # Optional user config (~1KB)
└── state.lz4            # Compressed binary state (20-30KB)
```

### In-Memory Strategy

**Load entire state on startup:**
1. Read `.legend/state.lz4` (20-30KB compressed)
2. LZ4 decompress → 50-100KB (~1ms)
3. Bincode deserialize → structs (~0.5ms)
4. Build lookup indices in-memory (~0.5ms)
5. **Total: <2ms**

**Why load everything?**
- At this scale, loading is faster than querying
- No complex index files needed
- Simpler code, fewer bugs
- Can build optimal indices on the fly

**In-memory indices (built on load):**
```rust
struct LegendState {
    features: Vec<Feature>,          // Main data

    // Fast lookup indices (built in <0.5ms)
    by_recency: Vec<usize>,          // Sorted by recency score
    by_file: HashMap<PathBuf, Vec<usize>>,   // file → features
    by_status: HashMap<Status, Vec<usize>>,  // status → features
}
```

---

## Design Constraints (To Maintain Performance)

### 1. No Complex Relationships
- Features can reference files (many-to-many)
- NO feature-to-feature dependencies
- NO nested hierarchies
- Keep data flat and simple

### 2. No Large Blobs
- Feature descriptions: <2KB (enforce truncation)
- No code snippets stored
- No diffs or patches
- Only metadata, not content

### 3. Aggressive Pruning
- Auto-archive features older than 90 days
- Remove stale file mappings
- Deduplicate paths
- Compress recency scores periodically

### 4. Atomic Writes Only
```rust
// Good: Atomic rename
fs::write(".legend/state.lz4.tmp", data)?;
fs::rename(".legend/state.lz4.tmp", ".legend/state.lz4")?;

// Bad: Non-atomic write (corruption risk)
fs::write(".legend/state.lz4", data)?;
```

### 5. Zero Network I/O
- Legend is 100% local
- No API calls, no telemetry
- Predictable latency

---

## Performance Testing

### Benchmarks to Maintain

**Must pass on every PR:**
```bash
# Load 1000 features, 3000 file mappings
cargo bench load_large_state
# Target: <3ms (p50), <5ms (p99)

# Update single feature
cargo bench update_feature
# Target: <2ms (p50), <3ms (p99)

# Query by recency (top 20 features)
cargo bench query_by_recency
# Target: <0.1ms (already in memory)
```

### Size Tests
```bash
# Generate max-size state (1000 features)
cargo test max_size_state
# Assert: compressed size <100KB

# Typical project state (50 features)
cargo test typical_size_state
# Assert: compressed size <30KB
```

### Test Data Profiles

**Small project (50 features, 200 files):**
- Uncompressed: 30KB
- Compressed: 12KB
- Load time: <1ms

**Medium project (200 features, 1,000 files):**
- Uncompressed: 80KB
- Compressed: 25KB
- Load time: <2ms

**Large project (1,000 features, 5,000 files):**
- Uncompressed: 200KB
- Compressed: 60KB
- Load time: <4ms

---

## When to Revisit This Design

**Switch to SQLite or more complex indexing if:**
- Typical state regularly exceeds 200KB uncompressed
- Load times exceed 5ms on average hardware
- Need partial updates (only modify subset of features)
- Need transactional semantics beyond atomic writes

**Current design is optimal for:**
- Small to medium projects (99% of use cases)
- <5ms latency requirement
- Simple, maintainable codebase
- Predictable performance

---

## Implementation Notes

### Rust Crates
```toml
[dependencies]
bincode = "1.3"      # Binary serialization
lz4 = "1.24"         # Fast compression
serde = { version = "1.0", features = ["derive"] }
```

### Serialization Pattern
```rust
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
struct Feature {
    id: String,
    name: String,
    status: FeatureStatus,
    files: Vec<PathBuf>,
    created_at: i64,
    last_updated: i64,
    recency_score: f64,
}

// All types must impl Serialize + Deserialize
// No custom serialization needed - bincode handles it
```

### Compression Pattern
```rust
use lz4::block::{compress, decompress};

// Save
let serialized = bincode::serialize(&state)?;
let compressed = compress(&serialized, None, true)?;
atomic_write(".legend/state.lz4", &compressed)?;

// Load
let compressed = fs::read(".legend/state.lz4")?;
let serialized = decompress(&compressed, None)?;
let state: LegendState = bincode::deserialize(&serialized)?;
```

---

## Monitoring Performance

### Log Slow Operations
```rust
let start = Instant::now();
let state = LegendState::load()?;
let elapsed = start.elapsed();

if elapsed.as_millis() > 5 {
    eprintln!("Warning: load took {}ms (target: <5ms)", elapsed.as_millis());
}
```

### Optional Profiling Flag
```bash
legend get_state --profile
# Output:
# Load:        1.8ms
# Decompress:  0.9ms
# Deserialize: 0.6ms
# Build index: 0.3ms
# Total:       1.8ms
```

---

## Summary

**Target:** <5ms per operation, <30KB on disk (typical project)

**Strategy:** Load entire state into memory, compress with LZ4, serialize with Bincode

**Limits:** Max 1,000 features, 5,000 files, 200KB uncompressed

**Why:** Legend is called every Claude message - must be imperceptible

**When to change:** If state regularly exceeds 200KB or loads take >5ms
