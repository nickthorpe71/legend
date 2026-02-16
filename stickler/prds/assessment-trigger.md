# Feature: Assessment Trigger

## Context
When users click "Ready for Assessment", the system must prepare and begin the mastery test. This is the transition from passive reading to active verification.

## Requirements
- "Ready for Assessment" button visible in reading view
- Button click triggers assessment generation
- Show loading state while questions are generated
- Navigate to assessment view once ready
- Display introductory message about mastery threshold (90%)
- Prevent starting assessment if already passed
- Allow retry if previously failed

## User Flow
- Step 1: User finishes reading chapter
- Step 2: User clicks "Ready for Assessment" button
- Step 3: Loading indicator appears ("Generating questions...")
- Step 4: Backend generates 5-10 questions via LLM
- Step 5: Assessment view loads with first question
- Step 6: User sees mastery threshold explanation (90%)

## Acceptance Criteria
- Given user clicks "Ready for Assessment", then loading indicator appears
- Questions are generated within 10 seconds
- User is navigated to /book/:id/chapter/:num/assessment
- First question is displayed immediately
- If chapter already passed, show "Already Mastered" message
- If previously failed, allow retry with new questions

## Technical Notes
- Button in ReadingView component triggers assessment
- POST /api/books/:id/chapters/:num/assessment/generate
- Backend calls LLM to generate questions
- Store questions in database/file with assessment_id
- Return assessment_id to frontend
- Navigate to AssessmentView component
- Check chapter mastery status before generating

## Test Data
- book_id: "test-book-123"
- chapter_num: 1

## Success Signals
- Generating
- Assessment
- mastery
- Question 1

## Failure Signals
- Error
- Failed to generate
- timeout
