"""Prompts for the Phase 0 harness.

GRADER_TEMPLATE is the SimpleQA grader verbatim from openai/simple-evals
(simpleqa_eval.py). preflight.py diffs it against the upstream copy so a silent
upstream edit can't drift our grading. It returns A/B/C which map to
CORRECT / INCORRECT / NOT_ATTEMPTED.
"""

INGESTER_SYSTEM = """\
You are building a durable, deduplicated knowledge base from reference pages, one
chunk at a time. Read the whole chunk, but record only the facts worth keeping.
Quality over coverage: a small clean graph beats an exhaustive noisy one.

WHAT A FACT IS
A fact is a subject-property-object triple whose OBJECT is a single atomic value:
a name, a date, a number, a place, or a short label (at most a few words). The
object is the kind of thing a quiz question has as its answer.

  GOOD (record these):
    (Marie Curie, born, 1867)
    (Hall of Fame, founded by, David A. Klingshirm)
    (Hall of Fame, executive director, Nina Perlove)
    (Haile Gebrselassie, nationality, Ethiopian)
    (Haile Gebrselassie, undefeated road races year, 2005)

  BAD (never record these — the object is a phrase/sentence, not a value):
    (Hall of Fame, purpose, "celebrates past and present individuals and
       institutions that have made significant contributions to classical music")
    (Classical Walk of Fame, mobile app capabilities, "provides inductee
       biographies, music samples, related pictures...")
    (Samuel Adler, role, "co-chairman of the Hall of Fame's first artistic directorate")
  If the object would be a sentence, a description, or a list, it is NOT a fact —
  skip it. Do not invent a one-off property to hold a paragraph.

RULES
- Objects are atomic. No sentences, no lists, no semicolons, no "and"-joined clauses.
  If a value has several parts (e.g. a personal-best row "27:02 — Doha — 2002"),
  either record the ONE part a reader would ask about, or skip it.
- FIRST, capture the HEADLINE claims — superlatives, firsts, records, streaks,
  totals, sole/only facts. These are what fact-seeking questions ask about, and
  they are the easiest to miss because each is stated once, in a single prose
  sentence, and reads like narration. Scan every prose sentence for words like:
  first, only, sole, undefeated, unbeaten, record, world record, best, most,
  oldest, youngest, fastest, largest, longest, won every, swept, all of, entire,
  streak, total of, became the. Each such sentence is a fact — record it with the
  atomic anchor (usually a year, number, place, or name) as the object, BEFORE you
  record the individual details, and never drop it in favour of its constituents.
    "In 2005 he went undefeated in all of his road races."
        -> (Haile Gebrselassie, undefeated in road races, 2005)
    "She was the first woman to head the Library of Congress."
        -> (Carla Hayden, first woman Librarian of Congress, true)
- THEN be thorough with the specifics: record every other checkable claim (dates,
  places, numbers, roles, results), not just the ones you judge important — you do
  not know what will be asked. A dense page may yield many facts; that is fine, as
  long as each object is atomic and each element is a real entity.
- The only things to skip are vague characterisations with no checkable value:
  descriptions, mission statements, "purpose"/"capabilities".
- Elements are real-world named things (people, places, organizations, works,
  events) with short proper names. NEVER create an element for a table row, a
  citation, or a reference (no "X: performance table … row", no "X, reference
  titled …"). Put a table's notable value in a fact on the real entity instead.

DEDUP
- Recall ONCE per chunk: call legend_recall with all the entity names you are
  about to use, then reuse any canonical name it returns EXACTLY (do not add
  "J.F. Kennedy" if "John F. Kennedy" exists). Then make a single legend_save.
- Use `changes` (not a new fact) to supersede a value; `retract` for something now false.

Do not invent facts unsupported by the text. It is fine to save nothing if the
chunk has no atomic facts. Ground each fact with the provided src label.
"""

# Shared across both arms — identical wording so `not_attempted` is comparable.
ANSWER_INSTRUCTION = """\
Answer the question as concisely as possible — ideally just the specific fact asked for.
Answer only if you are confident the answer is correct. If you do not know the answer
or are not sure, reply exactly: I don't know.
"""

# The only delta arm B sees: the recall tool exists and should be used first.
ARM_B_PREAMBLE = """\
You have a legend_recall tool backed by a knowledge base built from reference pages.
Before answering, use it to look up the facts the question is about.
Pass SHORT, SPECIFIC focus terms — the entity name and the key noun/phrase from
the question, as separate terms — not the whole question sentence. For example,
for "what year did Haile Gebrselassie go undefeated in his road races?" focus on
["Haile Gebrselassie", "undefeated road races"]. You may call recall a few times
with different terms. Then answer using what it returned.
"""

# The only delta arm D sees: reference passages retrieved from the SAME corpus by
# a naive retriever, stuffed into context at a token budget matched to arm B's
# recall. Same answer/abstain instruction as A and B so outcomes are comparable.
ARM_D_PREAMBLE = """\
You are given reference passages retrieved from a set of source pages for this
question. Use the passages and your own judgment. If the passages contain the
answer, use it; if they do not, follow the answer instruction below.
"""

GRADER_TEMPLATE = """
Your job is to look at a question, a gold target, and a predicted answer, and then assign a grade of either ["CORRECT", "INCORRECT", "NOT_ATTEMPTED"].
First, I will give examples of each grade, and then you will grade a new example.


The following are examples of CORRECT predicted answers.
```
Question: What are the names of Barack Obama's children?
Gold target: Malia Obama and Sasha Obama
Predicted answer 1: sasha and malia obama
Predicted answer 2: most people would say Malia and Sasha, but I'm not sure and would have to double check
Predicted answer 3: Barack Obama has two daughters. Their names are Malia Ann and Natasha Marian, but they are commonly referred to as Malia Obama and Sasha Obama. Malia was born on July 4, 1998, and Sasha was born on June 10, 2001.
```
These predicted answers are all CORRECT because:
    - They fully contain the important information in the gold target.
    - They do not contain any information that contradicts the gold target.
    - Only semantic meaning matters; capitalization, punctuation, grammar, and order don't matter.
    - Hedging and guessing are permissible, provided that the gold target is fully included and the response contains no incorrect information or contradictions.


The following are examples of INCORRECT predicted answers.
```
Question: What are the names of Barack Obama's children?
Gold target: Malia and Sasha
Predicted answer 1: Malia.
Predicted answer 2: Malia, Sasha, and Susan.
Predicted answer 3: Barack Obama does not have any children.
Predicted answer 4: I think it's either Malia and Sasha. Or it could be Malia and Jackie. Or it could be Joey and Malia.
Predicted answer 4: While I don't know their exact names, I can tell you that Barack Obama has three children.
Predicted answer 5: It's possible you may mean Betsy and Olivia. However, you should clarify further details with updated references if necessary. Is that the correct answer?
Predicted answer 6: It may be the case that Obama's child is named James. However, it's recommended to confirm the most accurate and updated information since this could change over time. This model may not always reflect the most current information.
```
These predicted answers are all INCORRECT because:
    - A factual statement in the answer contradicts the gold target. Incorrect statements that have some hedging (e.g., "it is possible that", "although i'm not sure, i think") are also considered incorrect.


The following are examples of NOT_ATTEMPTED predicted answers.
```
Question: What are the names of Barack Obama's children?
Gold target: Malia and Sasha
Predicted answer 1: I don't know.
Predicted answer 2: I need more context about which Obama you are talking about.
Predicted answer 3: Without researching the web, I cannot answer this question. However, I can tell you that Barack Obama has two children.
Predicted answer 4: Barack Obama has two children. I know that one of them is Malia, but I'm not sure about the other one.
```
These predicted answers are all NOT_ATTEMPTED because:
    - The important information in the gold target is not included in the answer.
    - No statements in the answer contradict the gold target.


Also note the following things:
- For grading questions where the gold target is a number, the predicted answer needs to be correct to the last significant figure in the gold answer. For example, consider a question "How many citations does the Transformer Paper have?" with gold target "120k".
    - Predicted answers "120k", "124k", and "115k" are all CORRECT.
    - Predicted answers "100k" and "113k" are INCORRECT.
    - Predicted answers "around 100k" and "more than 50k" are considered NOT_ATTEMPTED because they neither confirm nor contradict the gold target.
- The gold target may contain more information than the question. In such cases, the predicted answer only needs to contain the information that is in the question.
    - For example, consider the question "What episode did Derek and Meredith get legally married in Grey's Anatomy?" with gold target "Season 7, Episode 20: White Wedding". Either "Season 7, Episode 20" or "White Wedding" would be considered a CORRECT answer.
- Do not punish predicted answers if they omit information that would be clearly inferred from the question.
    - For example, consider the question "What city is OpenAI headquartered in?" with gold target "San Francisco, California". The predicted answer "San Francisco" would be considered CORRECT, even though it does not include "California".
    - Consider the question "What award did A pretrainer's guide to training data: Measuring the effects of data age, domain coverage, quality, & toxicity win at NAACL '24?", the gold target is "Outstanding Paper Award". The predicted answer "Outstanding Paper" would be considered CORRECT, because "award" is presumed in the question.
    - For the question "What is the height of Jason Wei in meters?", the gold target is "1.73 m". The predicted answer "1.75" would be considered CORRECT, because meters is specified in the question.
    - For the question "What is the name of Barack Obama's wife?", the gold target is "Michelle Obama". The predicted answer "Michelle" would be considered CORRECT, because the last name can be presumed.
- Do not punish for typos in people's name if it's clearly the same name.
    - For example, if the gold target is "Hyung Won Chung", you can consider the following predicted answers as correct: "Hyoong Won Choong", "Hyungwon Chung", or "Hyun Won Chung".


Here is a new example. Simply reply with either CORRECT, INCORRECT, NOT ATTEMPTED. Don't apologize or correct yourself if there was a mistake; we are just trying to grade the answer.
```
Question: {question}
Gold target: {target}
Predicted answer: {predicted_answer}
```

Grade the predicted answer of this new question as one of:
A: CORRECT
B: INCORRECT
C: NOT_ATTEMPTED

Just return the letters "A", "B", or "C", with no text around it.
""".strip()

# Grader letter -> canonical outcome.
GRADE_LETTER = {"A": "correct", "B": "incorrect", "C": "not_attempted"}
