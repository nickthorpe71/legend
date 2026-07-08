# C\*

C\* is a practical style for writing C. Understand your data, write the concrete
thing, compress only what repeats. It is the house style for the Legend oracle
(`legend.c`, `embed.c`) — a single-translation-unit C99 program built under
`-Werror`.

---

# Core Approach

## Data First

Start every design by asking what the data looks like and how it flows. Start
with the structs.

Flat structs over nested hierarchies. A tagged union (`struct` with an `enum`
tag) over a web of pointers — a `switch` on a tag is a visible, cheap branch;
pointer indirection hides the code path and chases cache lines.

The shape of the data determines the shape of the code. Get the data right and
the code writes itself; get it wrong and no amount of abstraction will save you.

Think about how data sits in memory. Contiguous beats pointer-chasing — a
growable array of plain values outperforms a graph of `malloc`'d nodes. Legend
keeps elements and relations in **flat, growable arrays** and refers to them by
their array index — a `u32` **handle**, never an address (§ below). Whether to
prefer array-of-structs or struct-of-arrays depends on access pattern: iterate
whole records → AoS; sweep one field across many → SoA. Measure when it matters.

## Concrete Then Compress

Write the inline, concrete solution first. Do not extract a function or macro for
_reuse_ until a pattern has appeared at least three times. A helper called from
one site for reuse is indirection, not abstraction. (Extracting for _readability_
— naming a block to clarify a long function — is always fine.)

A clear 50-line function is often better than five abstractions.

1. Make it work. 2. Make it correct. 3. Make it fast.

Do not skip steps. Do not design for hypothetical future requirements. Working
code reveals the real problem. Measure continuously.

## Explicit State

Functions take explicit inputs and return explicit outputs. Prefer pure functions
— inputs in, result out, no globals touched. Legend's "brain" (the tick
computation) is pure over the graph; I/O (snapshot load/write, the lock, stdio)
lives at the edges.

If a function performs I/O, allocates, or mutates shared state, the name says so.
A reader should not have to navigate the codebase to know what a function does:

```c
int  snapshot_load(Hypergraph *g, const char *store);   // I/O in the name
void tick_save(Hypergraph *g, const Submission *s, ...); // mutates g, said plainly
double clamp_unit(double x);                             // pure: in, out
```

Vague names (`process`, `handle`, `do_it`) are a smell — say what and on what.

---

# The C Subset

C is small but sharp. C\* leans on the safe, legible part of it.

## Types

Use fixed-width integers from `<stdint.h>` (typedef'd short: `u32`, `i64`, `u8`)
so sizes are never in doubt — sizes are load-bearing in the snapshot format.
Reach for `struct`, `enum`, and tagged unions. Flat arrays (`T *items; u32 len,
cap;`) are the default container.

Don't typedef a pointer to hide that it's a pointer, and don't wrap a `struct`
tag away — a reader should see the shape. Do typedef the integer newtypes that
prevent category errors (a handle is not a count).

## Handles, not pointers

Elements and relations are addressed by their array index — a `u32` handle —
never by a raw pointer into the array. A pointer dangles the moment the array
grows (`realloc` can move it); a handle stays valid across growth and survives a
save/load round-trip. Use a reserved sentinel (`NONE_U32 = 0xFFFFFFFF`) for
"absent," and validate a handle before you index with it. Resolve to a pointer
only for the span of a single operation, never store one.

## Memory

Ownership is explicit and local. Prefer a few large, long-lived allocations
(growable arrays, arenas) over many small ones. Grow arrays by doubling; free
them in one place at teardown. There is no hidden allocation — if a function
allocates, that's part of its contract, and every path frees or hands off.

Do not `free` what you did not allocate here, and do not double-free — pick one
owner. `memset(x, 0, sizeof *x)` to reset a struct to a known-empty state.

## Control Flow

`if`, `switch`, `for`, `while`, early `return`. Use `switch` over an `enum` for
state machines and tagged unions — it's the C answer to what other languages use
class hierarchies for. Keep arms short; extract a complex arm into a function.

`goto cleanup` is the idiomatic single-exit for multi-step functions that acquire
resources — one label, releases in reverse order. That is the *only* sanctioned
`goto`.

---

# Error Handling

Functions report failure by return code (`int`: 0 = ok) or a sentinel value
(`NULL`, `NONE_U32`), with results delivered through out-parameters. Check every
return you can act on; propagate the rest.

```c
if (!snapshot_load(&g, store)) return err(ERR_NO_STORE, "...");  // act on it
u32 e = resolve(g, name);            // NONE_U32 if unresolved
if (e == NONE_U32) { ... }
```

At the process boundary, Legend uses a **`setjmp` error trap**: a parse or
invariant failure `longjmp`s to the top-level handler, which prints a structured
JSON error (`{"code": ...}`) and exits non-zero — or, in the MCP server, returns
an `isError` result without taking the process down. This keeps the happy path
free of error plumbing while still failing loud and clean.

Every error must be actionable: a stable `code` a caller can branch on, not just
prose. **Handle each failure in exactly one place** — if the boundary trap
already handles a bad payload gracefully, don't also scatter defensive
`if`-guards upstream. No belt-and-suspenders.

---

# Determinism

The oracle must replay byte-identically. Two rules protect that:

- **Strict IEEE math — never `-ffast-math`.** Reordering or contracting
  floating-point ops breaks reproducibility (and the stability caps depend on
  exact rounding).
- **One clock seam.** Wall-clock time enters only through `LEGEND_NOW`; nothing
  else calls `time()`. Replays and golden tests set it and get identical output.

---

# Dependencies

The standard library and `libm`. That's the bar. Bundled assets (the embedding
model) are plain files loaded at runtime, not code-generated or network-fetched.
Adding anything else is a real decision with a real cost you must own.

---

# Build Discipline

- One translation unit per binary (`legend.c`, `embed.c`); no build system beyond
  a `cc` invocation.
- `-std=c99 -Wall -Wextra -Werror`. Warnings are errors, always.
- `check.sh` also builds under ASan/UBSan with `-fno-sanitize-recover`; it must be
  green before commit.

---

# Performance

Do not optimize before measuring. Focus on hot paths — most code does not need it.

Cache locality first: flat arrays in contiguous memory, handles over pointer
graphs. When a kernel genuinely dominates (the embedder's dot product), reach for
SIMD **locally** — a `__attribute__((target("avx2,fma")))` function compiled into
an otherwise-portable `-O2` build — not a global ISA flag.

Measure the whole program first (a shell timer on realistic input). If too slow,
isolate the piece with a `clock_gettime` bracket printed to **stderr** so it never
pollutes the JSON on stdout. Fix the widest cost. Measure again. Repeat.
