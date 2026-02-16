# Feature: Retry Failed Assessment

## Context
When users score below 90%, they need the ability to review and retry the assessment with newly generated questions.

## Requirements
- Show "Retry Assessment" button when score < 90%
- Display weak areas/concepts before retry
- Generate new questions (not same ones)
- Rephrase questions to test same concepts differently
- Track retry attempts count
- Allow unlimited retries
- Clear previous attempt data before new attempt
- Maintain question quality across retries

## User Flow
- Step 1: User fails assessment (score < 90%)
- Step 2: Results page shows weak areas
- Step 3: User sees "Retry Assessment" button
- Step 4: User clicks retry button
- Step 5: New questions are generated
- Step 6: Assessment restarts with fresh questions
- Step 7: Previous scores are archived, new attempt begins

## Acceptance Criteria
- Given score < 90%, then "Retry Assessment" button is shown
- When retry clicked, then new questions are generated
- New questions test same concepts with different phrasing
- Previous attempt is stored for analytics
- Retry counter increments
- User can retry unlimited times
- Each retry feels like a fresh assessment

## Technical Notes
- POST /api/assessments/:id/retry endpoint
- LLM generates new questions for same chapter
- Prompt LLM to rephrase, not repeat questions
- Store previous attempt with attempt_number
- Create new assessment instance for retry
- Frontend navigates to fresh assessment view
- Track concept weaknesses to focus new questions
- Consider adaptive difficulty based on previous performance

## Test Data
- previous_score: 75
- retry_attempt: 2
- weak_concepts: ["spaced repetition", "active recall"]

## Success Signals
- Retry Assessment
- Try Again
- New questions
- Attempt

## Failure Signals
- Error
- Failed to generate
- timeout
