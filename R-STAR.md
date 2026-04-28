# R* — The Way of No Way

R* is a practical style for writing Rust. Understand your data, write the concrete thing, compress only what repeats.

There is no fixed way. There is the problem, there are the fundamentals, then the solution.

---

# Core Approach

## Data First

Start every design by asking what the data looks like and how it flows through the program. Don't start with traits, interfaces, or architectural diagrams. Start with the structs.

Flat structs over nested hierarchies. `enum` variants over trait-object polymorphism — dynamic dispatch costs 10-15x more than a match arm and hides the actual code path. Choose containers by access pattern, not by habit.

If you cannot describe the data layout on a whiteboard in under a minute, the design is too complex. The shape of the data determines the shape of the code. Get the data right and the code writes itself; get it wrong and no amount of abstraction will save you.

Think about how the data is stored in memory. A `Vec<Entity>` where each entity holds its components is often better than separate systems connected by IDs — until [profiling](#how-to-measure) says otherwise. Contiguous data wins by default.

## Concrete Then Compress

Write the inline, concrete solution first. Do not extract a function, trait, or module for *reuse* until a pattern has appeared at least three times. A function called from one site for reuse is indirection, not abstraction. (Extracting for *readability* — naming a block of logic to clarify a long function — is always fine.)

A clear 50-line function is often better than five abstractions.

Compression means removing duplication that actually exists, not duplication you predict. Three similar blocks of code are a signal. Two are a coincidence. When you do extract, the new unit must justify itself: it must simplify every call site, not just centralize code.

Resist the urge to generalize early. The first implementation teaches you what the problem actually is. The second shows you what varies. The third reveals the real abstraction.

Steps:

1. Make it work.
2. Make it correct.
3. Make it fast.

Working code reveals the real problem.

Do not skip steps. Do not design for hypothetical future requirements. [Measure](#how-to-measure) continuously.

## Explicit State

Functions take explicit inputs and return explicit outputs. Write pure functions by default — they are testable, composable, and inherently thread-safe.

If a function performs I/O, allocates, or mutates shared state, the name says so:

```rust
fn load_state(path: &Path) -> Result<State>    // I/O is in the name
fn calculate_score(feature: &Feature) -> f64    // pure: inputs in, output out
```

Minimize nested conditionals. Flatten logic with early returns. Each function should have one job. A long function that does one thing sequentially is fine — a short function that does three things is not.

Names should be explicit and descriptive. A reader should not need to navigate the entire codebase to understand a function.

```rust
fn parse_feature_from_json(bytes: &[u8]) -> Result<Feature>    // clear
fn calculate_recency_score(days: f64) -> f64                    // clear
fn process(b: &[u8]) -> Result<Feature>                          // too vague
```

---

# The Rust Subset

Rust is a large language. Most programs require only a small part of it. R* defines a practical subset.

## Types

Core: `struct`, `enum`, `Option<T>`, `Result<T, E>`, `Vec<T>`, `&[T]`, `String`, `&str`, `[T; N]`.
Primitives: `i32`, `u32`, `usize`, `f32`, `f64`, `bool`.

These form the foundation of most code.

**`String` vs `&str`:** Accept `&str` in function parameters, store `String` in structs. Convert with `.to_string()` or `.as_str()`. This covers 90% of cases.

## Ownership and Lifetimes

Borrow by default. Clone when cost is acceptable and clarity improves.

```
fn process(data: &[Item]) -> Output     // borrow
fn update(state: &mut State)            // mutable borrow
```

Reasonable to clone: small structs, pipeline stages, lifetime simplification, concurrent tasks.
Avoid cloning: large buffers, large collections, hot loops.

Understand the cost. [Measure](#how-to-measure) if uncertain.

**Lifetimes.** Most lifetimes are inferred — you will not write them often. When the compiler asks for one, it means a reference in the output must be tied to an input:

```rust
fn first_word(s: &str) -> &str          // lifetime inferred: output lives as long as input
fn longest<'a>(a: &'a str, b: &'a str) -> &'a str   // explicit: both inputs must outlive output
```

If lifetime annotations start spreading through your code, restructure: return owned data, clone, or break the function apart. Lifetimes that require diagrams are a red flag.

## Control Flow

Tools: `if`, `match`, `for`, `while`, `return`.

Use `match` + `enum` for state machines and domain logic. This combination replaces most of what other languages use class hierarchies for, at a fraction of the runtime cost.

Keep match arms short — extract complex arm bodies into functions.

## Iterators

Core methods: `iter`, `map`, `filter`, `fold`, `collect`, `enumerate`, `zip`, `flat_map`. Everything else (`sum`, `count`, `any`, `all`, `find`) is a specialization of `fold` — use `fold` directly or use a loop.

Use closures when they remain small. Chain freely for filter-map-collect patterns. When a chain exceeds three methods or the logic branches, use a loop instead.

```rust
// iterator: shaping data
let active: Vec<&Feature> = features.iter().filter(|f| f.active).collect();

// loop: reasoning through logic
let mut result = Vec::new();
for feature in &features {
    if feature.status == Status::Complete && feature.score > threshold {
        result.push(feature);
    }
}
```

Loops for reasoning. Iterators for shaping data.

## Traits and Generics

Derive standard traits: `Debug`, `Clone`, `Default`, `PartialEq`. Use traits for small interface boundaries and library integration. Do not build trait hierarchies in application code.

Use generics in utility functions and libraries. Avoid complex generic signatures in application code — concrete types are easier to read, debug, and compile.

```rust
fn find_by_key<T, K: PartialEq>(items: &[T], key: K, f: fn(&T) -> K) -> Option<&T>
```

## Macros

Allowed: `println!`, `format!`, `vec!`, `assert!`, derive macros.

No macro DSLs. No proc macros in application code unless the alternative is significantly worse. Code must remain visible and understandable — if you cannot read the expanded output in your head, the macro is too complex.

---

# Data Design

Prefer flat structs with explicit fields:

```rust
struct Feature {
    id: FeatureId,
    name: String,
    status: Status,
}
```

Attach methods with `impl` blocks. Use `new()` as a constructor when initialization needs validation or defaults:

```rust
impl Feature {
    fn new(id: FeatureId, name: String) -> Self {
        Self { id, name, status: Status::Pending }
    }
}
```

Do not use builder patterns for simple types — a `new()` function with clear parameters is almost always sufficient.

Use newtypes to prevent confusion between semantically different values:

```rust
struct FeatureId(String);
struct Timestamp(i64);
```

Wrap where confusion causes bugs. Do not wrap everything.

Choose data structures by access pattern:

```
[T; N]        fixed-size, known at compile time
Vec<T>        ordered, dynamic
HashMap<K,V>  key-value lookup
HashSet<T>    uniqueness, membership tests
```

Do not default to `Vec` for everything. Think about how data will be queried, iterated, and mutated. The right container eliminates code; the wrong one generates it.

---

# Error Handling

No `unwrap()` in production code. Allowed in tests, prototypes, and proven-impossible states (with a comment explaining why).

Use `?` for propagation. Use `match` when recovery is possible. Do not over-handle errors — if you cannot do anything useful with an error, propagate it.

```rust
let file = File::open(path)?;                    // propagate
let config = parse(input).unwrap_or_default();   // recover with default
```

Application errors: `Result<T, Box<dyn Error>>` or a single app-level enum.
Library errors: explicit error enums with meaningful variants.

Keep error types simple. Every error message must be actionable — if a user cannot fix the problem from the message alone, the message is wrong.

## Option

`Option<T>` is how Rust represents "might not exist." Handle it explicitly:

```rust
if let Some(user) = users.get(id) {     // act on presence
    greet(user);
}
let name = config.name.unwrap_or_default();          // provide a fallback
let port = config.port.unwrap_or(8080);              // provide a specific default
let host = config.host.as_deref().unwrap_or("localhost");  // Option<String> → &str
```

Use `?` to propagate `None` in functions that return `Option`. Use `map` to transform the inner value without unwrapping. Do not chain more than two `Option` methods — use `if let` or `match` instead.

---

# Code Organization

Organize modules by responsibility, not by layer:

```
storage/        # how data persists
commands/       # what the program does
memory/         # domain logic
types.rs        # shared data definitions
```

Avoid `models/services/controllers/utils` layering. These names describe architectural roles, not what the code does.

Default visibility: `pub(crate)`. Expose only what must be public. A public interface is a long-term commitment — keep it minimal. Every public function is a promise to future callers.

Keep modules small enough to hold in your head. When a module grows large, look for a natural seam to split along. Split by responsibility, not by arbitrary size limits.

Prefer the standard library. Add external crates when they clearly reduce complexity or risk. Prefer widely used, well-maintained crates. Audit transitive dependencies — a crate that pulls in 50 sub-crates for a simple task is not simple.

You must be willing to take on the responsibility of any dependency you add.

---

# Testing

Put unit tests in the same file as the code they test, inside a `#[cfg(test)]` module at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_zero_for_empty_input() {
        assert_eq!(calculate_score(&Feature::default()), 0.0);
    }
}
```

Use `assert_eq!` for values, `assert!` for booleans. Name tests after the behavior they verify, not the function they call.

Put integration tests in `tests/` when you need to test the public API as an outside caller would. Keep test files focused — one file per area of behavior, not one file per source file.

Pure functions are trivially testable. This is another reason to prefer them.

---

# Performance and Concurrency

Do not optimize before [measuring](#how-to-measure). Focus on hot paths — most code does not need micro-optimization.

Cache locality matters. Flat data in contiguous memory outperforms pointer-chasing through heap allocations. Prefer `Vec<T>` over `Vec<Box<T>>` when T is small.

Use synchronous code by default. Introduce async only when real I/O concurrency benefits exist.

For shared state: `Arc<T>`, `Arc<Mutex<T>>`. Ownership transfer is usually simpler than lifetime gymnastics.

Pure functions are inherently thread-safe — prefer them over shared mutable state. The less mutable state exists, the fewer concurrency bugs are possible.

## How to Measure

Measure however you want, but be thorough. Here is one way that works well for most programs.

**Whole program.** Use wall-clock time with `hyperfine` or a simple shell timer. Run the program end-to-end on realistic input. This is your ground truth — if total runtime is acceptable, stop.

```bash
hyperfine --warmup 3 './target/release/my_program input.dat'
```

**Isolate a piece.** Wrap the code in question with `std::time::Instant`:

```rust
let start = Instant::now();
let result = do_expensive_work(&data);
let elapsed = start.elapsed();
eprintln!("expensive_work: {elapsed:?}");
```

Print to stderr so it does not pollute output. This is fast to add, fast to remove, and tells you exactly where time goes. When you need statistical rigor or regression tracking, move the isolated piece into a `criterion` benchmark.

**Find the hot path.** When you know something is slow but not where, use a profiler:

```bash
cargo flamegraph -- my_program input.dat
```

Read the flamegraph top-down. Wide bars are where time is spent. Optimize those. Ignore everything else.

**The loop.** Measure whole program. If too slow, isolate pieces with `Instant`. If unclear, profile with flamegraph. Fix the widest bar. Measure the whole program again. Repeat until done.
