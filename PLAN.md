# Legend Rust Implementation Plan

## Core Vision
Build Legend in Rust to learn systems programming while creating a production-quality, ultra-fast tool for AI-assisted development.

---

## Design Decisions (Finalized)

### Why Rust:
1. **Learn systems programming** - Ownership, lifetimes, zero-cost abstractions
2. **Memory safety** - No segfaults, data races caught at compile time
3. **Performance** - Meet <5ms latency requirement (see PERFORMANCE.md)
4. **Modern tooling** - Cargo, clippy, rustfmt, excellent ecosystem
5. **Production quality** - Type safety prevents entire classes of bugs

### Core Principles:
- **Performance critical** - <5ms per operation (Legend called every Claude message)
- **Explicit error handling** - Result<T, E> everywhere, no unwrap in production code
- **Minimal dependencies** - Only serde, bincode, lz4
- **Clear code** - Readable over clever
- **Teaching through best practices** - Production code with educational comments

### Storage Strategy:
- **Format**: Bincode (binary serialization) + LZ4 compression
- **Target**: <5ms load/save, <30KB on disk (typical project)
- **See PERFORMANCE.md** for detailed constraints and benchmarks

### Performance Architecture: Read-Optimized, Write-Heavy

**Key Insight:** AI writes are slow (Claude takes 5-30s to respond), reads must be instant.

**Read Path (<5ms hard requirement):**
- `legend get_state` - Called every session start
- Pre-computed: embeddings, indices, recency scores all calculated on write
- Just load + decompress + deserialize + return
- Target: <5ms, typically ~2-3ms

**Write Path (100-500ms acceptable):**
- `legend update` - Called after Claude finishes responding
- Expensive operations allowed: compute embeddings, rebuild indices, recalculate scores
- Budget: 100-500ms is imperceptible (Claude just spent 10+ seconds responding)

**This enables:**
- Semantic search via pre-computed embeddings (200ms to generate, 0ms to read)
- Complex indices built on write, instant lookup on read
- Rich metadata processing without impacting read performance

### Feature Metadata Strategy:

**Structured filtering (fast, required):**
- `domain` - Primary categorization ("auth", "storage", "api", "ui")
- `tags` - Flexible labels (["backend", "security", "database"])
- `status` - Current state (Pending, InProgress, Blocked, Complete)

**Rich context (for AI understanding):**
- `description` - What this feature does (required for embeddings)
- `context` - Why we're building it, background info (optional)

**Semantic search (future, Layer 9+):**
- `embedding` - Pre-computed 384-dim vector for semantic similarity
- Generated on write path (200ms), stored for instant read
- Enables "find features related to user security" without exact keyword matches

**Search flow:**
1. Filter by domain/tags (instant, <1ms)
2. Claude reads descriptions of filtered results (~10-50 features)
3. (Optional future) Semantic search within filtered subset

---

## Rust-Focused Layer Plan (8 Layers)

Each layer builds working functionality and teaches Rust concepts naturally.

### Layer 1: Basic CLI (30-45 min)
**What we're building:**
- Cargo project setup
- Command-line argument parsing
- Command routing with match
- Help message

**Rust concepts taught:**
- Project structure (main.rs, Cargo.toml)
- Vec<String>, &str vs String
- match expressions
- println! macro

**Test:** `cargo run -- help` works

---

### Layer 2: Core Types (45-60 min)
**What we're building:**
- Feature struct with metadata fields (domain, tags, description, context)
- FeatureStatus enum
- LegendState struct
- Constructor functions

**Rust concepts taught:**
- Structs and impl blocks
- Enums (not just C-style!)
- Option<T> for nullable fields
- pub/private visibility
- Vec<T> for dynamic arrays

**Fields added:**
- Required: `id`, `name`, `domain`, `description`, `status`
- Optional: `context` (Option<String>)
- Collections: `tags` (Vec<String>), `files_involved` (Vec<String>)
- Metadata: `created_at`, `last_updated`, `recency_score`

**Test:** Create Feature in code, verify it compiles

---

### Layer 3: Init Command (1 hr)
**What we're building:**
- Create `.legend/` directory
- Write initial config.toml
- Write empty state.lz4
- Basic error handling

**Rust concepts taught:**
- std::fs module (create_dir_all, File)
- Result<T, E> and the ? operator
- Error propagation
- Modules (commands/init.rs)

**Test:** `cargo run -- init` creates .legend directory

---

### Layer 4: Serialization (1.5 hr)
**What we're building:**
- Add serde derives
- Implement save/load with bincode
- Add LZ4 compression
- Atomic file writes

**Rust concepts taught:**
- Traits (Serialize, Deserialize)
- #[derive] macro
- External crates (add to Cargo.toml)
- Byte slices (&[u8])

**Test:** Save and load LegendState successfully

---

### Layer 5: Get State Command (30 min)
**What we're building:**
- Load state.lz4
- Print as JSON to stdout (for Claude)
- Handle missing file gracefully

**Rust concepts taught:**
- serde_json for output
- Converting between formats
- Borrowing (&) basics

**Test:** `cargo run -- get_state` prints JSON

---

### Layer 6: Update + Merge (2 hr)
**What we're building:**
- Read JSON from stdin
- Parse into Update struct
- Merge logic (update features, deduplicate)
- Recency score calculation
- Write back to disk

**Rust concepts taught:**
- std::io::stdin()
- HashMap for fast lookups
- Mutable references (&mut)
- Iterators and closures
- Time/date handling

**Test:** Update feature via stdin, verify merge

---

### Layer 7: Show Command (45 min)
**What we're building:**
- Load state
- Format human-readable table
- Sort by recency
- Color output (optional)

**Rust concepts taught:**
- String formatting
- Sorting with sort_by
- Iterator methods (map, filter)

**Test:** `cargo run -- show` displays nice output

---

### Layer 8: Bootstrap Discovery (2-3 hr)
**What we're building:**
- Walk directory tree
- Detect languages (file extensions)
- Detect potential features (directory patterns)
- Output discovery report
- Prompt Claude to interview user

**Rust concepts taught:**
- Recursive directory traversal
- Pattern matching on paths
- Building complex data structures
- Real-world Rust patterns

**Test:** Run on existing project, verify detection

---

## Progress Tracking

### Completed Layers: 5/8
- [x] Layer 1: Basic CLI
- [x] Layer 2: Core Types
- [x] Layer 3: Init Command
- [x] Layer 4: Serialization
- [x] Layer 5: Get State Command
- [ ] Layer 6: Update + Merge
- [ ] Layer 7: Show Command
- [ ] Layer 8: Bootstrap Discovery

---

## Build System

**Cargo** (Rust's build system and package manager):
- `cargo build` - Compile in debug mode
- `cargo build --release` - Optimized build
- `cargo run -- <args>` - Build and run with arguments
- `cargo test` - Run tests
- `cargo bench` - Run benchmarks (for performance validation)
- `cargo clippy` - Lint and suggestions
- `cargo fmt` - Auto-format code

Simple, batteries-included, no Makefile needed!

---

## Project Structure

```
legend/
├── Cargo.toml            # Project config and dependencies
├── Cargo.lock            # Locked dependency versions
├── src/
│   ├── main.rs           # CLI entry point
│   ├── types.rs          # Core structs (Feature, LegendState)
│   ├── storage.rs        # Save/load with bincode + lz4
│   ├── merge.rs          # State merging logic
│   ├── temporal.rs       # Recency scoring
│   ├── discover.rs       # Codebase scanning
│   └── commands/
│       ├── mod.rs        # Module declaration
│       ├── init.rs       # legend init
│       ├── get_state.rs  # legend get_state
│       ├── update.rs     # legend update
│       └── show.rs       # legend show
├── benches/
│   └── performance.rs    # Benchmarks (<5ms requirement)
├── tests/
│   └── integration.rs    # Integration tests
├── PLAN.md               # This file
├── PERFORMANCE.md        # Performance constraints
├── R*.md                 # Rust style guide
└── base_idea.md          # Core concept
```

---

## Dependencies

**Minimal and performance-focused:**

```toml
[dependencies]
serde = { version = "1.0", features = ["derive"] }
bincode = "1.3"
lz4 = "1.24"
serde_json = "1.0"  # For JSON output to Claude

[dev-dependencies]
criterion = "0.5"  # For benchmarking
```

That's it! Small, fast, well-maintained crates.

---

## Testing Strategy

**After each layer:**
1. Compile with `cargo build`
2. Run with `cargo run -- <command>`
3. Verify expected behavior
4. Check for warnings with `cargo clippy`
5. Format with `cargo fmt`

**Performance validation:**
- Run `cargo bench` to ensure <5ms targets met
- See PERFORMANCE.md for detailed benchmarks

---

## Learning Approach

**This is a collaborative learning project:**
- Claude explains concepts BEFORE writing code
- Code includes teaching comments
- User tests after each small change
- Questions encouraged at any time
- No rushing - understand before moving on

**See SESSION_GUIDE.md** for detailed workflow.

---

## Next Steps

1. ✅ Update all documentation to Rust
2. Create Cargo project (`cargo init`)
3. Set up basic project structure
4. Start Layer 1: Basic CLI

**Ready to start Layer 1 when you are!**
