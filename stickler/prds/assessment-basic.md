# Feature: Assessment Basic UI

## Context
Users take assessments one question at a time. The UI must present questions clearly and collect answers without distraction.

## Requirements
- Display one question at a time
- Show question number (e.g., "Question 2 of 7")
- Question text prominently displayed
- Text input area for answer
- "Submit Answer" button
- Disable button while submitting
- No ability to skip or go back (enforced progression)
- Clean, focused design

## User Flow
- Step 1: Assessment begins, first question loads
- Step 2: User reads question text
- Step 3: User types answer in text area
- Step 4: User clicks "Submit Answer"
- Step 5: Button shows loading state
- Step 6: Answer is submitted to backend
- Step 7: Next question loads (or results if complete)

## Acceptance Criteria
- Given assessment starts, then Question 1 is displayed
- Question text is readable and well-formatted
- Text area accepts multi-line answers
- Submit button is disabled when answer is empty
- While submitting, button shows loading state
- No navigation away from assessment is allowed
- Question counter shows current/total questions

## Technical Notes
- Create AssessmentView component in React
- GET /api/assessments/:id to fetch questions
- POST /api/assessments/:id/answers to submit each answer
- Track current question index in component state
- Validate answer is not empty before enabling submit
- Prevent browser back button during assessment
- Auto-focus text area on question load

## Test Data
- assessment_id: "test-assessment-123"
- question_text: "Explain the concept of spaced repetition in your own words."

## Success Signals
- Question
- of
- Submit Answer
- Next

## Failure Signals
- Error
- Failed
- timeout
