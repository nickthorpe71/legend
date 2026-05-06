---
name: Don't repeat default values in field docstrings
description: User wants default values in one place (the Default impl), not duplicated in docstrings
type: feedback
---

When a struct has an `impl Default` (or `#[derive(Default)]`), do **not** restate specific default numbers in the field docstrings (e.g., "Default 0.55", "Default 3 ticks").

**Why:** The `Default` impl is the single source of truth. Repeating numbers in docstrings creates two places to update; one will drift. The docstring drift then misleads readers.

**How to apply:**
- Document *rationale* and *behavior*, not numbers: "strict by default so X doesn't happen", "higher bar than Y because Z" — yes. "Default 0.85" — no.
- If a field's value is structurally meaningful (not a tunable knob — e.g., a fixed dimensionality), naming the value is fine because it isn't a default that would change.
- Applies to Legend's Rust code; same principle likely transfers anywhere else with idiomatic Default impls.
