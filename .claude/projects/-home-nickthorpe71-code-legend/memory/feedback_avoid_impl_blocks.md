---
name: No methods on types — functional style
description: User wants FP style in Legend; standard trait derives + Default are fine, but NEVER inherent methods on types
type: feedback
---

User wants Legend written in a functional style. Data structs hold data; behavior lives in free functions that take the data as arguments.

**Rules:**
- **OK:** Standard trait derives (`Clone, Copy, Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Default`) — these are language/ecosystem requirements and the alternative isn't free functions.
- **OK:** `impl Default for Foo { fn default() -> Self { ... } }` — explicit Default impls are acceptable when the spec defines specific default values that derive(Default) can't express.
- **NEVER:** Inherent methods like `impl Foo { fn do_thing(&self) -> ... }`. Write a free function `pub fn do_thing(foo: &Foo) -> ...` instead.
- **Case by case:** Other trait impls (`Display`, `From`, `Iterator`, `Serialize`) — justify each before adding.

**Why:** Aligns with R-STAR.md ("Data First. Don't start with traits, interfaces, or architectural diagrams. Start with the structs.") and the user's FP preference. Behavior in free functions is searchable, testable in isolation, and doesn't hide logic against types.

**How to apply:**
- When tempted to write `impl Foo { fn ... }`, write a free function in the same module instead.
- For "constructors", prefer free functions like `pub fn new_policy() -> Policy { ... }` unless `Default` semantics are appropriate (then use `impl Default`).
- For "transformations", free functions taking `&Foo` or `&mut Foo` and returning the result.
- This applies to Legend's Rust code; other repos / contexts may differ.
