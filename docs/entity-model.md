# Design — small intentional entities (names are nouns, claims are structure)

**Status:** SPEC. A reframe of the core save model (Nick, 2026-07-27): an element NAME
is a short noun **handle** for one intentional thing; a CLAIM — a relationship, a
decision, an event connecting things — is **structure** (facts between entity-nodes +
a summary), never a sentence crammed into a name. This kills prose-name pollution at
the root and makes the graph richer for traversal + co-activation
([[project_coactivation]] future work).

## The principle
- **Names are nouns.** Every element is named by its *subject/topic* — a short
  reference (usually 1–4 words) to one intentional entity. `trap spells`, `EVE
  currency`, `sim core`, `Epic`, `Fabled`, `act 1 depth cap`.
- **Claims are relationships.** "rename Epic to Fabled" is not a thing — it's
  `[Epic] —renamed_to→ [Fabled]`. Extract the entities (nouns) as elements, write the
  claim as a **fact** between them. A name that reads as a *claim* (a verb connecting
  entities) is a relationship mis-filed as a node.
- **Small + intentional.** Reify the *meaningful* nouns, not every token. This is the
  lean direction (few precise entities + durable facts), NOT sprawling reification —
  "intentional" is the guard against over-extraction.

## Why this is achievable (grounding, trial store)
Name length by kind shows the discipline already half-works:
- Entity kinds (`character`/`enemy`/`file`/`function`/`person`/`system`) already sit at
  **1–3 words** — noun-handles, correct.
- Claim-flavored kinds are *inconsistent*, not lost: `decision` has both `'trap spells'`
  (good) and `'act 1 tops out at blvl 9 with normalized depth'` (claim); `question` has
  `'spell unlock model'` (good) and `'legend trial issue: attrs silently reify…'`
  (prose). The model **can** produce handles; it just drops the discipline for claims.
- The `(no kind)` bucket = **641 elements (half the store), max 1124 chars** — the
  predicate / prose-value / changes.to sludge. The biggest cleanup target.

## Kinds resolution (the key design call)
**Keep the kinds** — a `decision`/`event`/`question` is a real, referenceable node. But
its **name becomes a noun handle** (the subject/topic), and its **claim becomes
structure**:
- `decision` "act 1 tops out at blvl 9…" → node `act 1 depth cap` (kind decision) +
  fact(s) carrying the value + a short reasoning summary. The `chose`/`rejected`/`reason`
  facts already exist — the *name* just stops being the claim.
- `event` "Session 2026-07-15 with Nick: batch-fixed 5 playtest notes" → node with a
  short handle (topic/date) + facts for what changed. (Events are the hardest — narrative
  by nature; the handle is the topic, the narrative goes to the summary, the concrete
  changes go to facts.)
- `question` → node named by its topic (`spell unlock model`), the question itself in
  the summary.

So decisions/events/questions don't vanish; their **names shorten to handles** and their
**content moves to facts + summary** — which the model already does for the good cases.

## The mechanism (two halves — instruction alone is proven dead)
1. **Instruction rewrite** (the core teaching), on all three surfaces —
   `MCP_INSTRUCTIONS`, the `legend_save` tool description, and the `onboard` prompt:
   > A NAME is a short noun — the *subject* of the thing (`trap spells`, `Epic`, `act 1
   > depth cap`). It is NEVER a claim, a sentence, a progress note, or a log. If your
   > name contains a verb or asserts something between two things, that's a CLAIM: name
   > the elements (the nouns) and write the claim as a FACT between them. Small,
   > intentional entities — reify the meaningful nouns, not every phrase.
2. **Mechanical gate** at every mint path — element names, fact objects, attr values
   (generalize the existing `changes.to` prose backstop to *all* name-minting sites).
   Reject a minted name that reads as a claim/prose, with guidance to decompose.
   - **Detection:** length/word is the pragmatic proxy the gate keys on; the finer
     noun-vs-short-claim call is carried by the instruction. **Threshold needs
     calibration** — entity kinds are ≤3 words, legit decision handles run 2–4 words,
     claims start ~5+ words / ~40+ chars, prose is unmistakable by ~8 words / ~60 chars.
     Calibrate against the trial store's *legit* noun-names to bound the false-positive
     rate before shipping (measure-first, like the changes.to gate). Consider
     **kind-awareness** (stricter for entity kinds, a touch looser for decision/event).
   - **Recovery** (make it cheap, or the gate thrashes): the error names the decomposition
     — "this name is a claim; save the nouns as elements and the claim as a fact:
     `facts:[{s,p,o}]`, put reasoning in the summary."

## Legacy cleanup (a gate stops NEW pollution; it doesn't remove OLD)
The existing prose/claim-named elements (the `(no kind)` bucket + long-named
decisions/events/questions) stay until restructured. Options:
- **LLM-driven, via the `maintain` prompt:** a gardening pass that finds claim-named
  elements and rewrites them into nouns + facts. Fits the human-in-the-loop maintenance
  model; the safest (semantics-preserving, reviewable).
- **Not a blind migration** — decomposing a claim into the right entities + fact is a
  semantic judgment, so it should be LLM-driven + reviewed, not a mechanical rewrite.
Scope: this pairs with the deferred **maintenance-cadence** gap — building the cleanup
is also building the gardener the fleet needs.

## Validation (measure-first, before it reaches the trial)
1. **Gate false-positive rate:** replay the trial store's element names through the gate
   — how many *legit* noun-handles would it reject? Target near-0; tune the threshold.
2. **Does the model comply?** Re-ingest a slice (or run a fresh onboard) with the new
   instruction + gate and measure: median name words/chars down toward the entity-kind
   baseline (1–3 words), `(no kind)` prose share down, WITHOUT a spike in element count
   (the over-extraction guard) or save rejections (the friction guard).
3. **Determinism gate green;** new reject path errors cleanly (fuzz), no golden regen for
   existing clean payloads.

## Risks
- **Friction / over-extraction** (the two the trial warns about): managed by "intentional"
  (lean) + good instruction examples + a not-too-aggressive gate. The re-ingest measure
  (2) is the check.
- **Gate blast radius:** it fires at *every* element mint — a mis-calibrated threshold
  rejects legit saves broadly. Calibration (1) + kind-awareness + adversarial review
  before shipping.
- **Kinds edge cases:** events are genuinely narrative; the handle/summary/facts split
  needs good examples or the model will keep logging into the name.

## Build order
1. Calibrate the gate threshold (measure legit-name false-positives) + decide kind-aware.
2. Instruction rewrite (all three surfaces) + the multi-path name gate + recovery message.
3. **Adversarial review** (core-model change, high blast radius) before it touches a store.
4. Validate on a re-ingest slice (compliance + no over-extraction/friction).
5. Legacy cleanup via `maintain` (also seeds the maintenance cadence).
