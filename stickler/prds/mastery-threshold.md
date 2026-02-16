# Feature: Mastery Threshold Evaluation

## Context
After all questions are answered and graded, the system calculates the overall score and determines if the user has reached the 90% mastery threshold.

## Requirements
- Calculate average score from all questions
- Compare to mastery threshold (90%)
- If ≥ 90%: Display "Chapter Mastered!" message
- If < 90%: Display "Review and Retry" with weak areas
- Show final score percentage
- Highlight which concepts were strong/weak
- Provide "Return to Chapters" or "Retry Assessment" button
- Update chapter status to "Mastered" if passed

## User Flow
- Step 1: User completes final question
- Step 2: System calculates overall score
- Step 3: Results page displays
- Step 4: If passed: "Chapter Mastered!" with final score
- Step 5: If failed: "Review Needed" with weak concepts listed
- Step 6: User clicks "Return to Chapters" (if passed) or "Retry" (if failed)

## Acceptance Criteria
- Given all questions answered, then final score is calculated
- When score ≥ 90%, then "Mastered" status is shown
- When score < 90%, then weak areas are identified
- Final score percentage is prominently displayed
- User can return to chapter list or retry assessment
- Chapter status is updated in database if mastered
- Next chapter is unlocked if this chapter is mastered

## Technical Notes
- Calculate average score from all question scores
- POST /api/assessments/:id/complete to finalize
- Backend updates chapter mastery status
- Unlock next chapter if current chapter mastered
- Identify weak concepts based on low-scoring questions
- Return results object with status, score, weak_concepts
- Frontend displays appropriate message based on status

## Test Data
- final_score: 92
- threshold: 90
- status: "mastered"

## Success Signals
- Mastered
- Chapter Complete
- score
- Passed
- Next Chapter

## Failure Signals
- Review Needed
- Below threshold
- weak areas
