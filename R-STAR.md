# R\*

R\* is a practical style for writing Rust. Understand your data, write the concrete thing, compress only what repeats.

---

# Core Approach

## Data First

Start every design by asking what the data looks like and how it flows through the program. Start with the structs.

Flat structs over nested hierarchies. `enum` variants over trait-object polymorphism — dynamic dispatch can cost an order of magnitude more than a match arm in hot loops (workload-dependent, varies with inlining and branch prediction) and hides the actual code path.

The shape of the data determines the shape of the code. Get the data right and the code writes itself; get it wrong and no amount of abstraction will save you.

Think about how the data is stored in memory. The rule is contiguous beats pointer-chasing — a `Vec<T>` of plain values outperforms `Vec<Box<T>>` or graphs of ID-linked heap objects. Whether to prefer array-of-structs (one `Vec<Entity>`) or struct-of-arrays (separate component vectors, ECS-style) depends on access pattern: iterate whole entities → AoS; iterate one field across many entities → SoA. [Profile](#how-to-measure) when it matters.

## Concrete Then Compress

Write the inline, concrete solution first. Do not extract a function, trait, or module for _reuse_ until a pattern has appeared at least three times. A function called from one site for reuse is indirection, not abstraction. (Extracting for _readability_ — naming a block of logic to clarify a long function — is always fine.)

A clear 50-line function is often better than five abstractions.

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

Names should be explicit and descriptive. A reader should not need to navigate the entire codebase to understand a function.

```rust
fn parse_feature_from_json(bytes: &[u8]) -> Result<Feature>    // clear
fn calculate_recency_score(days: f64) -> f64                    // clear
fn process(b: &[u8]) -> Result<Feature>                          // too vague
```

---

# The Rust Subset

Rust is a large language. Most programs require only a small part of it. R\* defines a practical subset.

## Types

Core: `struct`, `enum`, `Option<T>`, `Result<T, E>`, `Vec<T>`, `&[T]`, `String`, `&str`.

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
fn first_word(s: &str) -> &str          // lifetime elided: output lives as long as input
fn longest<'a>(a: &'a str, b: &'a str) -> &'a str   // explicit: both inputs must outlive output
```

If lifetime annotations start spreading through your code, restructure: return owned data, clone, or break the function apart. Lifetimes that require diagrams are a red flag.

## Control Flow

Tools: `if`, `match`, `for`, `while`, `return`.

Use `match` + `enum` for state machines and domain logic. This combination replaces most of what other languages use class hierarchies for, at a fraction of the runtime cost.

Keep match arms short — extract complex arm bodies into functions.

## Iterators

Core methods: `iter`, `map`, `filter`, `fold`, `collect`, `enumerate`, `zip`, `flat_map`.

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

# Data Design

Prefer flat structs with explicit fields:

```rust
struct Feature {
    id: FeatureId,
    name: String,
    status: Status,
}
```

Avoid methods and constructors in most cases.

Do:

```rust
let my_feature = Feature {
    id: FeatureId("f1".into()),
    name: "a name".to_string(),
    status: Status::Pending,
};
```

Over:

```rust
impl Feature {
    fn new(id: FeatureId, name: String) -> Self {
        Self { id, name, status: Status::Pending }
    }
}
```

Reach for `new` (or another named constructor) when there's a real reason: private fields that need a controlled construction path, invariants to validate, non-trivial defaults, or allocation/setup that callers should not repeat. Skip it when a struct literal says everything `new` would.

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

# Dependencies

Prefer the standard library. Add external crates when they clearly reduce complexity or risk. Prefer widely used, well-maintained crates. Audit transitive dependencies — a crate that pulls in 50 sub-crates for a simple task is not simple.

You must be willing to take on the responsibility of any dependency you add.

---

# Performance and Concurrency

Do not optimize before [measuring](#how-to-measure). Focus on hot paths — most code does not need micro-optimization.

Cache locality matters. Flat data in contiguous memory outperforms pointer-chasing through heap allocations. Prefer `Vec<T>` over `Vec<Box<T>>` when T is small.

Use synchronous code by default. Introduce async only when real I/O concurrency benefits exist.

For shared state: `Arc<T>`, `Arc<Mutex<T>>`. Ownership transfer is usually simpler than lifetime gymnastics. The less mutable state exists, the fewer concurrency bugs are possible.

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
