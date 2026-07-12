"""Prompts for the conflict_resolution benchmark.

The facts are deliberately counterfactual (e.g. "the capital of Romania is
Rajanpur"), so the ingester must record them as stated, and the consumer must
trust the store over its own world knowledge. Conflicts resolve latest-wins,
which is exactly Legend's `changes` semantics.
"""

CONFLICT_INGESTER_SYSTEM = """\
You are loading a list of atomic facts into a knowledge graph. Each line is
"N. <statement>" — one fact. Convert each statement to a triple and save it:
the subject and object are elements (people, places, teams, works, sports,
genres, positions...), the property is the relation.

Rules:
- Recall the entities before saving so you reuse canonical names and never mint a
  duplicate for the same thing.
- CONFLICTS ARE UPDATES. If a later line restates a subject+property you already
  stored but with a DIFFERENT object, the later line supersedes it — use `changes`
  {target, property, to} to update the current value. Do NOT keep both; the most
  recent statement is the current truth.
- These facts may contradict the real world (they are intentionally scrambled).
  Record them EXACTLY as written. Never "correct" a fact from your own knowledge.
- One fact per statement; objects are the atomic value stated (a name/place/word).
- Element summaries must be NEUTRAL — state only the kind ("Novel", "Person",
  "Sport", "Football club"). NEVER put a stated property into a summary (no author,
  capital, spouse, country...). Those values live ONLY in the fact/`changes`; your
  own knowledge of them is wrong here and a summary that repeats it will mislead the
  reader. When in doubt, give no summary and let the facts carry the meaning.

Save the facts from this chunk (recall first as needed), then stop.
"""

# Shared final instruction, identical across both arms.
ANSWER_INSTRUCTION = """\
Answer with just the specific value asked for — a name, place, or word — nothing else.
If you do not know, reply exactly: I don't know.
"""

# Arm B delta: the store holds the facts; trust it, and chain for multi-hop.
ARM_B_PREAMBLE = """\
You have NO reliable knowledge of these entities. Pretend you have never heard of
any person, place, work, or sport named in the question. Everything you "know"
about them is WRONG for this task and must not influence your answer.

The ONLY source of truth is the FACT TRIPLES returned by legend_recall — the
`attrs` objects under `recent`, `related`, and `resolution`, each shaped like
{subject, <property>: <value>}. IGNORE every element `summary`; summaries may
echo real-world facts that the triples deliberately overwrite.

Hard rules:
- The triple always wins. If a triple says {rugby union, created in country: India},
  then rugby union was created in India — answer "India", NOT England, no matter
  how certain you feel otherwise.
- Never introduce a name, place, or value that did not appear as the OBJECT of a
  triple you actually retrieved. If you catch yourself typing a real-world answer
  you weren't handed by a triple, stop — it is wrong.
- Multi-hop: copy the object string VERBATIM into your next recall. Given
  {Our Mutual Friend, author: Charles Darwin}, your next recall focus MUST be
  "Charles Darwin" — never the real-world author. Then read that entity's triples
  for the next link, and so on until you reach the final value.
- Do NOT answer until every link of the chain has been resolved through triples.

Your final answer is the last triple's object, copied verbatim.
"""
