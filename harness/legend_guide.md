# Legend — operating manual for the calling model

This is the orientation a Legend MCP server + session hook give a model so it can
use Legend well. It is the model's mental model of the tool.

## What Legend is

Legend is your long-term memory across sessions. It is **not** a chat log — it is a
deduplicated, revisable knowledge graph. Two kinds of thing live in it:

- **Elements** — durable named things: a project, function, module, system, mechanic,
  parameter, constraint, decision, spell, enemy, character, place, or open question.
  Each has a canonical **name**, a **kind**, optional **aliases**, and a one-line **summary**.
- **Facts** — a relationship or value as a triple **{s, p, o}**: subject, property, object.
  `s` and `o` are element names or literal values; `p` is the property/relationship
  (e.g. `{"s":"mana economy","p":"starting_mana","o":"0"}`).

## The one discipline that matters: recall before you save

Before recording anything, **recall the entities involved**. This tells you the exact
canonical name memory already uses for a thing and its current facts. Then:

- **Reuse the existing canonical name.** Never mint a second element for a thing that
  already exists ("mana economy" one turn, "mana system" the next fragments memory and
  makes it unrecallable). If memory has it, use its name verbatim.
- **Update by re-asserting.** To change a value, save the NEW value with the **same
  subject + property**. Legend supersedes the old value automatically and keeps it in
  history — you never delete, you supersede.
- **Honor retraction.** If a note says a fact was retracted / withdrawn / was wrong, do
  **not** save it. If memory already holds something now known to be false, don't re-assert it.
- **Save durable facts, not chatter.** Record what a future session needs; skip the ephemeral.

## Reading a recall frame

`recall` with a `focus` returns a frame: the resolved focus element(s) with their summaries,
related elements, current facts, and store-wide sections (`state`, `decisions`, `constraints`,
`open`, `recent`). Notes on reading it:

- If a focus **resolves**, you get the element + its live facts. Use them.
- If it does **not** resolve confidently, you get `resolved: false` and a **candidate list**
  — pick the right one by name/summary; a low top score means "probably not here."
- **Superseded** values appear only in history; **retracted** ones never appear.
- If something isn't in the frame, memory doesn't have it. **Do not invent it** — say so.

## Answering from memory

When answering a question, use **only** what recall surfaces. If the answer isn't in the
frame, reply that it's not in memory rather than guessing. A confident wrong answer is worse
than an honest "not recorded."
