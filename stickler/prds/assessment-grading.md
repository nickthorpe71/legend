# Feature: Assessment Grading

## Context
After each answer is submitted, an LLM evaluates the response and provides a score and feedback. This happens immediately to maintain engagement.

## Requirements
- Submit answer to grading endpoint
- LLM evaluates answer against expected understanding
- Return score (0-100) and feedback
- Display feedback immediately after submission
- Show score for the question
- Update running total score
- Proceed to next question automatically after feedback
- Store graded answers for concept tracking

## User Flow
- Step 1: User submits answer
- Step 2: Loading indicator while grading
- Step 3: Feedback appears (score + explanation)
- Step 4: User reads feedback (3-5 seconds auto-delay)
- Step 5: Next question loads automatically
- Step 6: Process repeats until all questions answered

## Acceptance Criteria
- Given answer submitted, then LLM grades within 5 seconds
- Feedback includes score (0-100) and explanation
- Score is added to running total
- Feedback is clear about what was correct/incorrect
- After brief delay, next question loads automatically
- All answers and scores are stored for later analysis

## Technical Notes
- POST /api/assessments/:id/answers/:questionId
- Backend calls LLM with question, expected answer, user answer
- LLM returns score and feedback
- Store answer, score, feedback in database
- Frontend displays feedback in modal or inline
- Auto-advance to next question after 5 seconds
- Calculate cumulative score for mastery threshold

## Test Data
- question_id: "q1"
- user_answer: "Spaced repetition helps memory by reviewing at intervals"
- expected_score_range: 70-90

## Success Signals
- Score
- Feedback
- Correct
- Good answer
- Next question

## Failure Signals
- Error
- Grading failed
- timeout
