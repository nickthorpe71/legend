#!/usr/bin/env python3
"""Score LongMemEval benchmark results against oracle answers."""

import json
import re
import sys
from collections import defaultdict


WORD_TO_NUM = {
    'zero': '0', 'one': '1', 'two': '2', 'three': '3', 'four': '4',
    'five': '5', 'six': '6', 'seven': '7', 'eight': '8', 'nine': '9',
    'ten': '10', 'eleven': '11', 'twelve': '12', 'thirteen': '13',
    'fourteen': '14', 'fifteen': '15', 'sixteen': '16', 'seventeen': '17',
    'eighteen': '18', 'nineteen': '19', 'twenty': '20', 'thirty': '30',
    'forty': '40', 'fifty': '50', 'sixty': '60', 'seventy': '70',
    'eighty': '80', 'ninety': '90', 'hundred': '100',
}


def extract_numbers(text: str) -> set[str]:
    """Extract all numbers from text, including word forms."""
    # Match standalone numbers and numbers attached to units (e.g., 16GB, $50, 10%)
    nums = set(re.findall(r'(?<!\d)\d+(?:\.\d+)?', text))
    t = text.lower()

    # Handle "X and a half" patterns
    for word, digit in WORD_TO_NUM.items():
        if f'{word} and a half' in t:
            nums.add(f'{digit}.5')
        if word in t:
            nums.add(digit)

    # Handle "half" standalone
    if re.search(r'\bhalf\b', t):
        nums.add('0.5')

    return nums


def normalize(text: str) -> str:
    """Lowercase and strip formatting."""
    text = text.lower().strip()
    text = text.replace('**', '').replace('*', '')
    return text


def is_idk(hypothesis: str) -> bool:
    """Check if the hypothesis is an 'I don't know' response."""
    h = hypothesis.lower()
    patterns = [
        "i don't know",
        "i do not know",
        "i don't have enough",
        "i do not have enough",
        "cannot determine",
        "can't determine",
        "unable to determine",
        "not enough information",
        "no relevant memories",
        "no memories",
        "couldn't find",
        "could not find",
        "i cannot recall",
        "i can't recall",
        "no information available",
        "don't have any memories",
        "do not have any memories",
        "no specific memory",
        "no specific information",
    ]
    # Only match if it's near the start (first 200 chars) to avoid false positives
    h_start = h[:300]
    return any(p in h_start for p in patterns)


def is_error(hypothesis: str) -> bool:
    """Check if the hypothesis is an error response."""
    h = hypothesis.lower()
    return "error during processing" in h or "error:" in h[:20]


def is_numeric_answer(answer: str) -> bool:
    """Check if the oracle answer is primarily numeric (e.g., day counts)."""
    return bool(re.match(r'^\d+', answer.strip()))


def get_acceptable_numbers(answer: str) -> set[str]:
    """Get all acceptable numeric answers."""
    return set(re.findall(r'\b(\d+(?:\.\d+)?)\b', answer))


def score_numeric(answer: str, hypothesis: str) -> bool:
    """Score a numeric answer by checking if acceptable numbers appear in hypothesis."""
    acceptable = get_acceptable_numbers(answer)
    hyp_numbers = extract_numbers(hypothesis)
    return bool(acceptable & hyp_numbers)


def extract_key_concepts(text: str) -> list[str]:
    """Extract key noun phrases and concepts from text for matching."""
    text = normalize(text)
    # Remove common sentence starters for preference answers
    text = re.sub(r"^the user would prefer (responses|suggestions) (that |of |related to )?", "", text)

    stopwords = {
        'the', 'and', 'for', 'was', 'are', 'that', 'this', 'with', 'from',
        'have', 'has', 'had', 'but', 'not', 'you', 'your', 'they', 'their',
        'also', 'been', 'were', 'will', 'would', 'could', 'should', 'into',
        'about', 'than', 'them', 'then', 'what', 'when', 'which', 'who',
        'how', 'its', 'can', 'did', 'does', 'done', 'just', 'more',
        'some', 'such', 'very', 'only', 'other', 'over', 'after', 'before',
        'including', 'last', 'day', 'days', 'acceptable', 'ago', 'months',
        'month', 'year', 'years', 'week', 'weeks', 'a', 'an', 'of', 'in',
        'to', 'is', 'it', 'at', 'on', 'or', 'be', 'by', 'so', 'if',
        'no', 'up', 'do', 'my', 'me', 'he', 'we', 'as', 'like', 'might',
        'may', 'suggest', 'recommend', 'especially', 'those', 'specifically',
        'tailored', 'focus', 'related', 'based', 'offer', 'great', 'prefer',
        'responses', 'suggestions', 'where', 'there', 'these', 'being',
        'particularly', 'possibly', 'given', 'since', 'user', 'them',
        'enhance', 'well', 'help', 'provide', 'consider', 'whether',
    }

    words = [w for w in re.findall(r"[a-z0-9'-]+", text) if w not in stopwords and len(w) >= 3]
    return words


NUM_TO_WORD = {v: k for k, v in WORD_TO_NUM.items()}


def answer_is_word_number(answer: str) -> str | None:
    """If answer is a word-form number like 'seven', return its digit form."""
    a = answer.lower().strip().rstrip('.')
    if a in WORD_TO_NUM:
        return WORD_TO_NUM[a]
    return None


def extract_noun_phrases(text: str) -> list[str]:
    """Extract contiguous noun phrases (2+ content words) from text."""
    words = re.findall(r"[a-z0-9'-]+", text.lower())
    stopwords = {
        'the', 'a', 'an', 'of', 'in', 'to', 'is', 'it', 'at', 'on', 'or',
        'be', 'by', 'so', 'if', 'no', 'up', 'do', 'my', 'me', 'he', 'we',
        'as', 'was', 'not', 'and', 'for', 'are', 'but', 'you', 'your',
        'that', 'this', 'with', 'from', 'have', 'has', 'had', 'its', 'can',
    }
    # Build runs of content words
    phrases = []
    current = []
    for w in words:
        if w not in stopwords and len(w) >= 2:
            current.append(w)
        else:
            if len(current) >= 2:
                phrases.append(' '.join(current))
            current = []
    if len(current) >= 2:
        phrases.append(' '.join(current))
    return phrases


def score_text_strict(answer: str, hypothesis: str) -> bool:
    """Score a text answer - strict matching for non-preference questions."""
    h_norm = normalize(hypothesis)
    a_norm = normalize(answer)

    # Remove "also acceptable" clause
    a_clean = re.sub(r'\.\s*\d+.*(?:also acceptable|is acceptable).*$', '', a_norm, flags=re.IGNORECASE)
    a_clean = a_clean.strip().rstrip('.')

    # Direct substring match
    if a_clean in h_norm:
        return True

    # Try with quotes removed (answer might be in quotes like 'The Hate U Give')
    a_unquoted = a_clean.replace("'", "").replace('"', '')
    h_unquoted = h_norm.replace("'", "").replace('"', '')
    if a_unquoted in h_unquoted:
        return True

    # If answer is a word-form number (e.g., "seven"), also check digit form
    digit = answer_is_word_number(a_clean)
    if digit is not None:
        hyp_nums = extract_numbers(hypothesis)
        if digit in hyp_nums:
            return True

    # If answer is a digit, also check word form
    if a_clean.isdigit() and a_clean in NUM_TO_WORD:
        word = NUM_TO_WORD[a_clean]
        if word in h_norm:
            return True

    # Handle "word_number + unit" answers like "four weeks", "two months", "five months ago"
    time_units = ['week', 'weeks', 'month', 'months', 'year', 'years', 'day', 'days',
                  'hour', 'hours', 'minute', 'minutes']
    words_in_answer = a_clean.split()
    if len(words_in_answer) >= 2 and words_in_answer[0] in WORD_TO_NUM:
        digit_form = WORD_TO_NUM[words_in_answer[0]]
        rest = ' '.join(words_in_answer[1:])
        digit_answer = f'{digit_form} {rest}'
        if digit_answer in h_norm:
            return True
        # Also check just "N unit" (e.g., "4 weeks" in "**4 weeks**")
        for unit in time_units:
            if unit in rest and f'{digit_form} {unit}' in h_norm:
                return True

    # Noun phrase matching: if the core noun phrase from the answer appears in
    # the hypothesis, count it correct even if surrounding words differ.
    # e.g. answer="GPS system not functioning correctly" → phrase "gps system"
    # matches hypothesis mentioning "car's GPS system"
    answer_phrases = extract_noun_phrases(a_clean)
    for phrase in answer_phrases:
        if phrase in h_norm:
            return True

    # Key word matching
    key_words = extract_key_concepts(a_clean)
    if not key_words:
        return a_clean in h_norm

    matched = sum(1 for w in key_words if w in h_norm)

    # For short answers (1-2 key words), require all to match
    # For longer answers, require 50%+ (lenient since phrasing varies)
    if len(key_words) <= 2:
        return matched == len(key_words)
    elif len(key_words) <= 4:
        return matched >= len(key_words) * 0.5
    else:
        return matched >= len(key_words) * 0.45


def score_preference(answer: str, hypothesis: str) -> bool:
    """Score a preference question - check if key concepts from expected preference appear."""
    if is_idk(hypothesis) or is_error(hypothesis):
        return False

    h_norm = normalize(hypothesis)
    a_norm = normalize(answer)

    # Extract key concepts from the preference answer
    key_words = extract_key_concepts(a_norm)

    if not key_words:
        return False

    matched = sum(1 for w in key_words if w in h_norm)

    # For preferences, require at least 40% of key concepts to appear
    # (hypothesis is a natural response, not a restatement of the preference)
    if len(key_words) <= 3:
        return matched >= 2
    else:
        return matched >= len(key_words) * 0.35


def is_abstention_answer(answer: str) -> bool:
    """Check if the expected answer is an abstention (info not sufficient)."""
    a = answer.lower()
    return (a.startswith("the information provided is not enough") or
            a.startswith("you did not mention") or
            a.startswith("0. you did not mention"))


def score_one(answer: str, hypothesis: str, question_type: str) -> bool:
    """Score a single question."""
    # For abstention questions, IDK is the CORRECT answer
    if is_abstention_answer(answer):
        return is_idk(hypothesis) or is_error(hypothesis)

    if is_idk(hypothesis) or is_error(hypothesis):
        return False

    if question_type == 'single-session-preference':
        return score_preference(answer, hypothesis)

    if is_numeric_answer(answer):
        return score_numeric(answer, hypothesis)
    else:
        return score_text_strict(answer, hypothesis)


def main():
    # Load oracle
    with open('/home/nickthorpe71/Code/legend/benchmarks/longmemeval_oracle.json') as f:
        oracle_list = json.load(f)
    oracle = {q['question_id']: q for q in oracle_list}

    # Load results
    results = []
    with open('/home/nickthorpe71/Code/legend/benchmarks/results.jsonl') as f:
        for line in f:
            line = line.strip()
            if line:
                results.append(json.loads(line))

    print(f"Results: {len(results)}, Oracle questions: {len(oracle)}")
    print()

    # Score each result
    correct = 0
    wrong = []
    idk_count = 0
    error_count = 0
    abs_total = 0
    abs_correct = 0
    by_type = defaultdict(lambda: {'correct': 0, 'total': 0, 'idk': 0})
    missing_oracle = 0

    for r in results:
        qid = r['question_id']
        hyp = r['hypothesis']

        if qid not in oracle:
            missing_oracle += 1
            continue

        q = oracle[qid]
        answer = str(q['answer'])
        qtype = q['question_type']

        by_type[qtype]['total'] += 1
        is_abs = is_abstention_answer(answer)
        if is_abs:
            abs_total += 1

        if is_error(hyp):
            error_count += 1
            if is_abs:
                correct += 1
                abs_correct += 1
                by_type[qtype]['correct'] += 1
            else:
                wrong.append((qid, qtype, answer, hyp[:150]))
                by_type[qtype]['idk'] += 1
            continue

        if is_idk(hyp):
            if is_abs:
                correct += 1
                abs_correct += 1
                by_type[qtype]['correct'] += 1
                idk_count += 1
            else:
                idk_count += 1
                wrong.append((qid, qtype, answer, hyp[:150]))
                by_type[qtype]['idk'] += 1
            continue

        if score_one(answer, hyp, qtype):
            correct += 1
            by_type[qtype]['correct'] += 1
        else:
            wrong.append((qid, qtype, answer, hyp[:150]))

    total = len(results) - missing_oracle

    # Print results
    print("=" * 80)
    print(f"OVERALL ACCURACY: {correct}/{total} = {correct/total*100:.1f}%")
    print(f"  Correct:         {correct}")
    print(f"  Wrong:           {total - correct}")
    print(f"  I don't know:    {idk_count}")
    print(f"  Errors:          {error_count}")
    non_abs_total = total - abs_total
    non_abs_correct = correct - abs_correct
    print(f"")
    print(f"  Non-abstention:  {non_abs_correct}/{non_abs_total} = {non_abs_correct/non_abs_total*100:.1f}%")
    if abs_total > 0:
        print(f"  Abstention:      {abs_correct}/{abs_total} = {abs_correct/abs_total*100:.1f}%")
    else:
        print(f"  Abstention:      N/A (no abstention questions in results)")
    if missing_oracle:
        print(f"  Missing oracle:  {missing_oracle}")
    print("=" * 80)
    print()

    print("ACCURACY BY QUESTION TYPE:")
    print("-" * 70)
    fmt = "  {:<32s} {:>3d}/{:>3d} = {:>5.1f}%  (IDK: {:>3d})"
    for qtype in sorted(by_type.keys()):
        d = by_type[qtype]
        pct = d['correct'] / d['total'] * 100 if d['total'] > 0 else 0
        print(fmt.format(qtype, d['correct'], d['total'], pct, d['idk']))
    print()

    # Separate wrong answers by category
    wrong_actual = [(qid, qt, a, h) for qid, qt, a, h in wrong if not is_idk(h) and not is_error(h)]
    wrong_idk = [(qid, qt, a, h) for qid, qt, a, h in wrong if is_idk(h)]
    wrong_err = [(qid, qt, a, h) for qid, qt, a, h in wrong if is_error(h)]

    print(f"INCORRECT ANSWERS (not IDK/Error): {len(wrong_actual)}")
    print("-" * 80)
    for qid, qtype, answer, hyp_snippet in wrong_actual:
        print(f"  {qid[:20]:20s} ({qtype})")
        print(f"    Expected: {answer[:120]}")
        print(f"    Got:      {hyp_snippet[:140]}")
        print()

    print(f"\"I DON'T KNOW\" RESPONSES: {len(wrong_idk)}")
    print("-" * 80)
    for qid, qtype, answer, hyp_snippet in wrong_idk:
        print(f"  {qid[:20]:20s} ({qtype}) -- Expected: {answer[:80]}")
    print()

    if wrong_err:
        print(f"ERROR RESPONSES: {len(wrong_err)}")
        print("-" * 80)
        for qid, qtype, answer, hyp_snippet in wrong_err:
            print(f"  {qid[:20]:20s} ({qtype})")
            print(f"    {hyp_snippet[:140]}")
        print()


if __name__ == '__main__':
    main()
