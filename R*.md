# R* - The Rust Way of No Way

> *"In strategy, it is important to see distant things as if they were close and to take a distanced view of close things."*
> — Miyamoto Musashi, The Book of Five Rings

---

## The Principle

**Adapt to the terrain.**

There is no fixed style. There is no "proper Rust way." There are fundamentals, and there is the problem in front of you.

Master the basics. Favor simplicity. Reach for power only when the need is clear—you will know when the time comes. Use macros when macros are needed (it is rare). Use traits when traits clarify (not before). Clone when cloning serves the code.

Do not follow dogma. Do not write code to impress other Rust programmers. Write code that works, that is clear, that can be changed.

If you fight the borrow checker for an hour, you are on the wrong path. Restructure. Own the data. Move forward.

Measure continuously. Progress in small steps. Favor working code over perfect code.

This is the way*.

---

## The Ground - Foundation

### On Simplicity

**Small is knowable.**

- Small functions (one purpose)
- Small modules (one responsibility)
- Small APIs (one obvious path)

Prefer fewer moving parts over framework patterns. A 50-line function you understand beats 5 abstractions you don't.

### On Clarity

**Local over global.**

The reader should understand each function without jumping across the codebase. If you must jump to understand, the abstraction has failed.

Names are long and boring:
```rust
fn parse_feature_from_json(bytes: &[u8]) -> Result<Feature>
fn calculate_recency_score(days_since_update: f64) -> f64
fn write_state_atomic(path: &Path, state: &State) -> Result<()>
```

Not:
```rust
fn parse(b: &[u8]) -> Result<F>  // What are we parsing? What is F?
fn score(d: f64) -> f64          // Score of what?
fn write(p: &Path, s: &S) -> R   // Too generic, too clever
```

### On Control

**Deterministic over magical.**

No hidden work. No surprising side effects. No action at a distance.

If a function modifies global state, allocates memory, or performs I/O, the name and signature must make this clear.

```rust
fn load_state() -> Result<State>           // Clear: I/O happens
fn calculate_score(feature: &Feature) -> f64  // Clear: pure computation
```

### On Invariants

**Make invalid states unrepresentable.**

Use the type system. If a value must be non-empty, the type should enforce it. If a state machine has three states, use an enum with three variants.

```rust
// Bad: Can represent invalid state
struct Feature {
    id: Option<String>,  // Why optional? ID is required
}

// Good: Cannot construct invalid feature
struct Feature {
    id: String,  // Required, enforced by constructor
}

impl Feature {
    fn new(id: String) -> Result<Self> {
        if id.is_empty() {
            return Err("Feature ID cannot be empty".into());
        }
        Ok(Feature { id })
    }
}
```

---

## The Way of Data - Types and Ownership

### On Ownership

**Ownership is a tool, not a religion.**

Borrowing is preferred. Cloning is not evil. Choose based on the situation:

**Borrow when:**
- Data stays in one scope
- Function only reads the data
- Performance matters and data is large

**Clone when:**
- Data passes through multiple functions
- Lifetime complexity obscures logic
- Data is small and clone cost is negligible

**Mind the cost.** Cloning a `String` is cheap. Cloning a 10MB `Vec<u8>` is not. Profile. Measure. Decide.

```rust
// Borrow: data used once, stays local
fn print_feature(feature: &Feature) {
    println!("{}", feature.name);
}

// Clone: data travels through pipeline
fn process_features(features: Vec<Feature>) -> Vec<ProcessedFeature> {
    features.into_iter()
        .map(|f| process(f))  // Owns f, transforms it
        .collect()
}
```

**For concurrency:** `Arc<Mutex<T>>` is the default. `Arc` for shared ownership, `Mutex` for shared mutation. Do not fight the borrow checker with lifetimes across threads—own the data.

### On Types

**Flat and simple.**

Avoid builders. Avoid complex constructors. Prefer plain structs with public fields or simple `new()` functions.

```rust
// Good: Direct construction
struct Feature {
    id: String,
    name: String,
    status: Status,
}

impl Feature {
    fn new(id: String, name: String) -> Self {
        Feature {
            id,
            name,
            status: Status::Pending,
        }
    }
}

// Also good: Public fields if validation isn't needed
struct Point {
    pub x: f64,
    pub y: f64,
}

let p = Point { x: 1.0, y: 2.0 };
```

**Newtypes prevent bugs:**
```rust
struct FeatureId(String);
struct Timestamp(i64);

// Now you cannot accidentally pass a Timestamp where a FeatureId is expected
fn get_feature(id: FeatureId) -> Option<Feature> { /* ... */ }
```

Use newtypes when they prevent mistakes. Do not wrap everything "just in case."

### On Data Structures

**Think before you reach for `Vec`.**

`Vec` is not always the answer. Consider:

- **Fixed size?** Use array: `[T; N]`
- **Small and bounded?** Use `SmallVec` or stack allocation
- **Key-value lookup?** Use `HashMap`
- **Ordered iteration?** Use `Vec`
- **Unique values?** Use `HashSet`

Choose the structure that matches the access pattern. Do not default to `Vec` because it is common.

```rust
// Known size at compile time
const MAX_FEATURES: usize = 100;
let features: [Feature; MAX_FEATURES] = /* ... */;

// Unknown size, grows dynamically
let features: Vec<Feature> = vec![];
```

---

## The Way of Flow - Control and Errors

### On Errors

**No `unwrap()` in production code. Period.**

Allowed only in:
- Tests
- Prototypes (marked clearly)
- Truly impossible states (with comment explaining why)

```rust
// Bad: Panic on error
let file = File::open(path).unwrap();

// Good: Propagate error
let file = File::open(path)?;

// Good: Handle error explicitly
let file = match File::open(path) {
    Ok(f) => f,
    Err(e) => {
        eprintln!("Failed to open {}: {}", path.display(), e);
        return Err(e.into());
    }
};
```

**On `expect()`:** Acceptable for impossible states if you explain why:
```rust
// OK: We just inserted this key, it must exist
let value = map.get(&key).expect("key was just inserted");
```

But consider: could you restructure to avoid the expectation?

**Error types:** Keep them simple. For applications, `Box<dyn Error>` is fine. For libraries, use custom enums. Add context at boundaries.

```rust
// Application: simple is fine
fn run() -> Result<(), Box<dyn Error>> { /* ... */ }

// Library: custom error type
#[derive(Debug)]
enum LegendError {
    Io(std::io::Error),
    InvalidState(String),
    CorruptedData { path: PathBuf, reason: String },
}
```

### On Match

**Match is for scannable control flow.**

Arms should be short. Logic belongs in functions, not in match arms.

```rust
// Good: Clean, scannable
match command {
    Command::Init => init()?,
    Command::Load => load()?,
    Command::Save => save()?,
    Command::Show => show()?,
}

// Bad: Logic buried in arms
match command {
    Command::Init => {
        let path = PathBuf::from(".legend");
        if !path.exists() {
            fs::create_dir_all(&path)?;
        }
        let state = State::default();
        write_state(&path.join("state.lz4"), &state)?;
        println!("Initialized");
    }
    // ... more buried logic
}

// Better: Extract to functions
match command {
    Command::Init => handle_init()?,
    // ...
}

fn handle_init() -> Result<()> {
    let path = PathBuf::from(".legend");
    if !path.exists() {
        fs::create_dir_all(&path)?;
    }
    let state = State::default();
    write_state(&path.join("state.lz4"), &state)?;
    println!("Initialized");
    Ok(())
}
```

**Exhaustiveness is a feature.** Let the compiler tell you when you forgot a case.

### On Iterators vs Loops

**Iterators are zero-cost abstractions.** They compile to the same code as a `for` loop. Use them when they clarify intent.

```rust
// Iterator: Clear intent, composable
let completed: Vec<_> = features
    .iter()
    .filter(|f| f.status == Status::Complete)
    .collect();

// For loop: Fine when iterators obscure
let mut completed = Vec::new();
for feature in &features {
    if feature.status == Status::Complete {
        completed.push(feature);
    }
}
```

**Choose based on clarity, not performance.** Both are equally fast. If the iterator chain becomes hard to read (3+ chained methods with complex closures), use a `for` loop.

**Cognitive cost matters:** A reader unfamiliar with `flat_map` and `filter_map` will struggle. Use judgment. Favor the clearer form.

---

## The Way of Code - Organization

### On Modules

**Split by logical grouping, not by layer.**

Organize by *what the code does*, not by *what kind of thing it is*.

```rust
// Good: Grouped by purpose
mod storage;     // All state persistence
mod commands;    // CLI command handlers
mod temporal;    // Recency scoring
mod types;       // Core domain types

// Bad: Grouped by abstraction
mod models;      // Just data?
mod services;    // What is a service?
mod controllers; // Framework thinking
mod utils;       // Junk drawer
```

**One responsibility per module.** Storage does not do parsing. Network does not do storage. Keep boundaries clean.

**Flat over nested.** Prefer `storage.rs` over `storage/mod.rs` unless the module is large (500+ lines) and benefits from subdivision.

### On Public APIs

**Most things are `pub(crate)`. `pub` is earned.**

Expose one obvious path to use your code. Hide implementation details. A large public API is a maintenance burden and a promise you must keep.

```rust
// Minimal public surface
pub fn load_state() -> Result<State> { /* ... */ }
pub fn save_state(state: &State) -> Result<()> { /* ... */ }

// Internal helpers stay private
fn compress(data: &[u8]) -> Vec<u8> { /* ... */ }
fn atomic_write(path: &Path, data: &[u8]) -> Result<()> { /* ... */ }
```

### On `main`

**Keep `main` thin.** It wires dependencies and calls `run()`.

```rust
fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let command = parse_command(&args)?;
    execute_command(command)?;
    Ok(())
}
```

---

## The Way of Progress - Performance and Iteration

### On Performance

**Measure first. Optimize second.**

Do not optimize prematurely. Build it working. Then measure. Then optimize where it matters.

**For Legend:** <5ms per operation is the requirement. Measure continuously. If a change slows things down, understand why before proceeding.

**Use the right tool:**
- Profiling: `cargo flamegraph`, `perf`
- Benchmarking: `criterion`
- Quick timing: `std::time::Instant`

```rust
let start = Instant::now();
let state = load_state()?;
let elapsed = start.elapsed();

if elapsed.as_millis() > 5 {
    eprintln!("Warning: load_state took {}ms", elapsed.as_millis());
}
```

**Inline judiciously.** The compiler usually knows better. Use `#[inline]` only after profiling proves the need.

**String types:**
- `&str` - Borrowed, use for reading
- `String` - Owned, use when you need to build/modify
- `Cow<str>` - When you might borrow or own (rare, only when needed)

Default to `&str` for parameters. Return `String` when you create new data.

### On Dependencies

**Minimize external crates.**

Default to stdlib. Add a dependency only when it clearly reduces risk or effort.

**Prefer boring crates:** Widely used, well-maintained, not magical.

- **Good:** `serde`, `bincode`, `lz4`, `clap`
- **Questionable:** Macro-heavy frameworks, niche crates, "clever" abstractions

**Avoid macro DSLs** unless essential. Macros that generate code make debugging harder and hide control flow. Operator overloading is fine—it adds clarity (`+` for addition, `*` for deref). Macros that build mini-languages are not.

### On Iteration

**Progress in small steps.**

1. Make it work
2. Make it right
3. Make it fast

Do not skip step 1. A working solution tells you what the problem actually is. Premature abstraction is waste.

**Run it. Test it. Measure it.**

Every change should be testable immediately. If you cannot run the code and see the result, you are too far from reality.

---

## The Way of Threads - Concurrency

**Sync by default. Async when needed.**

Use async only when:
- Real I/O concurrency benefit exists
- The complexity is justified

For Legend: Everything is sync. No need for async—operations complete in <5ms.

**For shared state:**
- `Arc<Mutex<T>>` - Default for shared mutable state across threads
- `Arc<T>` - For shared immutable state (clone once, share read-only)
- `Mutex<T>` - For single-threaded shared mutation (rare)

Do not fight the borrow checker with lifetimes across threads. Own the data with `Arc`.

```rust
use std::sync::{Arc, Mutex};

let state = Arc::new(Mutex::new(State::default()));

// Clone the Arc (cheap), share ownership
let state_clone = state.clone();
thread::spawn(move || {
    let mut s = state_clone.lock().unwrap();
    s.update();
});
```

---

## Red Flags

Avoid these patterns unless the need is clear and proven:

- Builder patterns for simple structs
- Traits with complex generic bounds (`where` clauses spanning multiple lines)
- Lifetimes that require diagrams
- Nested `if let` or `match` pyramids (flatten with early returns)
- Unwraps in production paths
- "This will scale later" architecture (build for today's problem)
- Macros that generate code (use functions)
- Dependencies that bring in 50 transitive crates

---

## The Litmus Test

Before finalizing code, ask:

**Can a tired engineer understand this in 60 seconds?**

If no, simplify.

**Is the public API minimal?**

If no, hide more.

**Are errors actionable?**

If no, add context.

**Did we avoid unnecessary abstraction?**

If no, inline it.

**Can we run it and measure it?**

If no, we are lost.

---

## For the AI Agent

When generating R* code:

1. **Smallest working solution first.** No architecture "just in case."
2. **Explicit over implicit.** Explicit types, explicit errors, explicit control flow.
3. **No builders unless proven.** Use simple constructors or public fields.
4. **Clone pragmatically.** If it makes code clearer and cost is low, clone.
5. **Explain tradeoffs briefly, then choose.** Sync vs async, borrow vs clone—state it, pick one, move on.
6. **Measure, don't guess.** If performance matters, show how to measure.

**On git commits:**
- Do not add "Generated by Claude" or "Co-Authored-By: Claude" to commit messages
- The README.md already discloses AI-generated code
- Keep commits concise and technical

---

## Summary

There is no fixed way. There is the problem, and there are fundamentals.

Master the basics. Favor simplicity. Reach for power only when the need is clear.

Adapt to the terrain.

This is R*.
