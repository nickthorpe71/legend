# The self anchor

Legend is written by exactly one class of author: the model that reads it back.
Until 2026-08-02 that author had no element. This adds one — `me` — so the agent
can appear in its own memory as a participant rather than only as an invisible
narrator.

## Why: the store already spoke in first person with nothing to point at

> **CORRECTION 2026-08-04.** The "30 elements" figure below came from a broken
> measure and is overstated by roughly 4x. It counted the human's pronouns
> quoted inside summaries, proper nouns (*Who I Would Have Been* is a spell), and
> `\bI\b` matching the I in `I/O`. The genuine population is **~7 across the
> whole month-long trial** — see the round 9 close in `alchamancer-trial.md`.
> The cases below are real and the argument stands, but the SCALE does not: this
> is a rare, high-value event class (~1/month), not a pervasive one. That is why
> round 9 could not measure it.

Measured on the live alchamancer2 store (787 elements, 2908 relations):

- **30 elements** carry `I`/`me`/`my`/`we` in a **name or summary**, outside any
  quoted source string. They had no referent. *(See correction above: ~7.)*
- The pronouns did not even share one. `#381` *"just let **me** select it from
  the menu"* is Nick; `#412` *"correcting **my** third push for survivability"*
  is the agent; `#525` *"better than **my** One True Sentence"* is the agent.
- `#458` is a **constraint that binds the agent** — *"from now on let me
  validate if an animation is good before **you** delete it"* — with no subject
  to bind to. It was stored as a floating rule.

`new_foundation.md` §11.7 already requires deictic spans to *resolve to an
existing element* instead of minting new nodes. For first and second person
there was no element to resolve to, so the rule was unsatisfiable. This is the
missing referent it presupposes — a coherence fix, not a new feature.

The other half is provenance. `source` + `src` are **1379 of 2908 relations
(47%)**, and 89 of the 174 distinct source objects are raw utterance strings
("Nick, 2026-08-01: \"...\"", mean 89 chars, max 235). Each is a speech act —
speaker, addressee, force, content — flattened into one opaque name. The speaker
was already a node (`#44 Nick Thorpe`, the store's only `person`). The addressee
never was.

## What it is

A single element, seeded into every store:

```
name     me
aliases  the assistant, the agent, myself
summary  The agent whose memory this store is: the model that reads and
         writes it, across every session. First person in a fact resolves here.
```

**It is vocabulary, not content** — the deictic anchor, the same category as
`subject` and `source`. That is what justifies its treatment:

- seeded at **salience 0**, so the hub itself never enters the orientation
  packet, the embed list, or recall candidates, while facts *about* it do;
- **no kind**, like the rest of the vocabulary;
- **protected** by `is_core_vocab` — it cannot be renamed or merged away,
  because every claim binding the agent resolves through it;
- excluded from `overview.active` explicitly, since it carries a summary *and*
  subjects facts and would otherwise qualify as a "topic" on both counts.

### Why a deictic word is safe as a canonical name

Normally `me` would be exactly the kind of name Legend forbids. It is safe here
for one specific reason: **a store has only one class of writer.** Nick does not
write to Legend; the agent does. So `me` has a single possible referent, and it
stays correct for every future session that reads it back — which is the whole
"treat Legend as *its own* memory" thesis.

Human first person does occur in the store, but only *inside quoted source
strings*, which are opaque text, never references. The apparent collision is
between a reference and a quotation — different layers.

The aliases are **not** for disambiguation; there is nothing to disambiguate.
They exist so a session reaching for a different surface form lands on this node
instead of minting a second self. `Claude` is deliberately **not** among them: a
store may need to talk about the model as a subject in its own right.

## How to write it

The shape is the hyper-relational one already decided (triple + qualifiers on
the statement), so nothing new is needed in the substrate:

```json
{"facts": [
  {"s": "Nick Thorpe", "p": "asked", "o": "me"},
  {"attrs": {"subject": "me", "removes": "em dash", "from": "text"}}
]}
```

yields

```
rel:N {subject: Nick Thorpe, asked: me}
rel:M {subject: me, removes: em dash, from: text}
```

Note the second fact uses the full `attrs` form: a fact is *either* `s/p/o` or
`attrs`, never `s/p/o` plus extra slots.

### The bright line

Include the self **only where it is a distinguishing participant** — where the
fact would be different if another agent had done it:

- a directive that binds the agent (`#458`),
- a correction it was given,
- an error it repeated (`#412`),
- a standing commitment it now holds.

**Do not anchor authorship on `me`.** Every save is the agent's, so recording
that fact on all of them costs 40% of the relation count and buys nothing.
`source` stays the material drawn on. This is the mega-hub failure mode, and
gauge [6] exists to catch it.

## What it unblocks

`#157 prior-miss count` is open and asks whether surfacing a prior-miss count
would let the *first* error suffice, rather than the second. That requires
knowing the agent erred before, which was not expressible. This is its
prerequisite.

## Honest scope

This is **representational correctness** plus that one unblock. It is *not* a
retrieval win — the recorded retrieval lever is still summary bloat and
over-extraction (`#66`, `docs/substrate-literals.md`). It should be graded as
the former.

## Migration

`seed_self()` is resolve-or-mint, run at init and on every load, exactly as
`seed_ext_vocab()` is. A store written before the anchor existed gains it on
first open: **exactly one new element, zero new relations, no migration step.**
Verified against a copy of the live 787-element trial store, and asserted in
`test_self_anchor`.

Fresh stores now seed **43** elements (32 core + 10 extended + self) rather than
42, so content ids start at `#43`. Element ids past the legacy core were never a
contract — the extended vocabulary is documented as not sitting at fixed ids —
but the golden fixtures hard-coded them, so 76 refs shifted by one.

## Gauges

`harness/bundle_gauges.py` reads three, because the failure modes pull in
opposite directions:

| gauge | baseline (2026-08-02) | reading |
|---|---|---|
| `[6]` self-anchored live facts | **0** (0.0%) | dead node if it stays 0; mega-hub if it passes 5% |
| `[6]` self on `source`/`src` | **0** | must stay 0 — authorship never anchors here |
| `[7]` unresolved first person | **30** | should fall as deixis gets resolved at write time |
| `[8]` provenance share | **1402 (48.3%)** | must not rise; the anchor replaces nothing yet |

Gauge `[7]` excludes quoted source strings. An earlier hand count of 39 included
them and was wrong for this purpose.
