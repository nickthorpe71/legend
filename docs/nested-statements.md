# Nested statements

Nick, 2026-08-02, looking at the chain `nick -> asked -> me -> negate -> em dash
-> from -> text`:

> we don't really have relations showing up like this. And really it's a special
> kind of relation because it's a statement. [...] The sum of the parts are
> greater than the whole.

He was right on the measurement and right about the defect. This documents what
was verified, what was built, and the one thing that still blocks adoption.

## The observation is correct, measured

On the live alchamancer2 store (2908 relations), relations that reference another
relation split cleanly in two:

| shape | count | what it is |
|---|---|---|
| `subject: rel:N` | **1209** | a meta *about* a statement — provenance (1167 `source`), modality, supersession |
| non-subject slot → `rel:N` | **37** | `derived_from` (31) + `supersedes` (6) — **all machine-minted plumbing** |

**Zero content-bearing nested statements.** Not one model has ever written "X
asked that [statement]". The substrate has supported it since `TERM_REL` existed;
nothing ever used it.

## What flattening destroys

The chain was first encoded as two sibling facts:

```
rel:10 {subject: Nick Thorpe, asked: me}
rel:11 {subject: me, removes: em dash, from: text}
```

Two things are lost, and both matter:

1. **Containment.** `rel:10` no longer says *what* was asked. Retract `rel:11`
   and the ask dangles pointing at nothing.
2. **The pivot.** `me` is the object of `asked` and the subject of `removes` —
   the *same* node in two roles. Side-by-side facts assert both roles but never
   that they are one.

There is also a truth bug: `rel:11` asserts as standing fact that the agent
removes em dashes, when that was only *requested*. The `non_actual` modal exists
for exactly this and was not being used.

## Refinement: it is nesting, not order

Nick's framing was that what makes it a statement is **order**. The evidence says
the load-bearing properties are **nesting** (one statement is an *argument* of
another) and **the pivot** (a shared node across roles), not sequence:

- Legend's slots are role-labeled (`subject`/`asked`/`content`), so role is
  already encoded without order — and dedup deliberately hashes the attr *set*.
- A bare arrow chain is ambiguous about grouping. Read `... removes -> em dash ->
  from -> text` strictly and you cannot tell whether `from` is another hop or a
  qualifier on `removes`. The first encoding had to guess, and guessed qualifier.

Order *does* matter, but as nesting **depth**: "A asked B to ask C" is a
different tree from "C asked B to ask A". Tree, not list.

## This is the general case of the provenance problem

`"Nick, 2026-08-01: \"13 should be an attack\""` **is** `said(Nick, [should_be(13,
attack)])` — a nested statement flattened into a string because there was no way
to write it. That is 89 opaque source elements, and `source`/`src` are **47% of
all relations**. Nick's chain is not a side case; it is the same missing
primitive the self anchor only touched the edge of (`docs/self-node.md`).

## What was built

The substrate needed nothing. Two changes, both on the read side:

**1. Frames expand a nested statement inline** (`frame_put_rel_attrs_n`):

```json
{"subject": "Nick Thorpe", "asked": "me",
 "content": {"ref": "rel:10",
             "attrs": {"subject": "me", "removes": "em dash", "from": "prose"},
             "modal": ["non_actual"]}}
```

The inner statement's **modal is carried**, because that is the entire difference
between "asked for" and "is true" — the same inversion `frame_put_rel_entry`
already guards against for top-level facts. Status is emitted when not
`asserted`, so a retracted inner statement cannot read as live.

Expansion is bounded by `FRAME_NEST_MAX` (2): depth 1 is a directive, depth 2 is
"A asked B to ask C", and past that the pointer stands so no chain can balloon a
frame. Orientation-packet size is already the live constraint (`#91`).

`subject: rel` is **not** expanded — it is a meta, already carried by its own
section, and expanding it would inline the target beneath all 1167 source metas.
`derived_from`/`supersedes` are excluded as versioning bookkeeping.

**2. Focus reaches a container from any term inside it.** `meta_by_target`
already indexed every `TERM_REL` slot, so the reverse link existed. Focusing
`em dash` — a term that appears *only* in the inner statement — now surfaces the
ask. Without this the request is invisible from the only term the reader has.

**`dump` stays flat.** It is the archival view where every relation is already
listed on its own, and the harness scripts parse that shape.

## The adoption blocker: `content` needs two saves

A `content` slot takes a `rel:` id, and a relation minted in the *same* payload
cannot be referenced — `{"content": "rel:10"}` fails with `unknown_ref` because
the id does not exist at plan time. So writing a directive is:

1. save the inner statement → read its id from `writes.minted_relations`
2. save the outer statement referencing it

Models save once per turn. **This is friction that will suppress adoption**, and
it means a round-10 reading of "nobody nested" would be ambiguous between *the
shape is unnatural* and *the flow is too hard*.

Gauge `[10]` exists to separate those: a speech act (`asked`/`told`/`requested`
…) whose object is the agent but that carries no `content` slot is a directive
with its content dropped — models reaching for the shape and not completing it.

**Candidate fix, not built:** a third ref form alongside `#<n>` (element) and
`rel:<n>` (relation) — `facts[<n>]`, referencing a relation minted earlier in the
same payload. Resolution already dispatches on ref form, and the write path
already defers same-payload *element* refs through pends; this extends that to
relations. It was deliberately not attempted here: the plan/apply machinery is
where all four of round 8's save-path bugs lived (`e6973ae`, `78416af`,
`bbc9c8c`, `f76d3e6`), and it deserves its own scrutiny rather than being tacked
onto a read-side change.

Deciding it on round-10 evidence is also the better experiment: if `[9]` rises
at current ergonomics, the ref form is unnecessary; if `[9]` stays 0 while `[10]`
climbs, the ref form is justified by measurement instead of guess.

## Vocabulary

`content` is **not** seeded. Instruction clause (12) names it as the canonical
slot; first use mints it like any predicate. Seeding would have canonicalized it
and protected it from merge, but it would also have shifted every content id a
second time in one cycle (fresh stores went 42 → 43 for the self anchor), and the
seed does not actually prevent sprawl — the instruction does. Revisit if round 10
shows competing slot names (`that`, `asks`, `body`).

## Gauges

| gauge | baseline | reading |
|---|---|---|
| `[9]` nested content statements | **0** | rises iff adopted |
| `[9]` inner marked `non_actual` | **0 of 0** | the directive shape done right |
| `[10]` speech acts with no content | **0** | high while `[9]` is low ⇒ the two-save flow is the blocker |

Plus `round_report.py`'s existing **packet bytes** — expansion makes frames
bigger, and bloat is a live concern.

**Baselines drift: re-take them at the re-pin.** The trial store is live and
moved measurably during the two hours this was built (provenance 1402 → 1461,
unresolved first person 30 → 31, bloat 168 → 187). Numbers recorded here are from
2026-08-02 and are illustrative of *shape*, not the grading baseline. Round 10's
baseline must be captured at the boundary, immediately before the re-pin.
