# Feature: Chapter Unlock on Pass

## Context
When a user masters a chapter (≥90%), the next chapter must be automatically unlocked and visible in the chapter list.

## Requirements
- Detect when assessment passes mastery threshold
- Update current chapter status to "Mastered"
- Unlock next chapter (set status to "Unlocked")
- Update chapter list UI to reflect changes
- Show visual confirmation of unlock
- Maintain lock on chapters beyond next chapter
- Handle final chapter (no next chapter to unlock)

## User Flow
- Step 1: User completes assessment with ≥90% score
- Step 2: System marks current chapter as "Mastered"
- Step 3: System unlocks next chapter
- Step 4: User returns to chapter list
- Step 5: Current chapter shows "Mastered" badge
- Step 6: Next chapter now shows "Unlocked" (no lock icon)
- Step 7: User can click next chapter to begin reading

## Acceptance Criteria
- Given user passes assessment, then current chapter is marked "Mastered"
- When chapter is mastered, then next chapter is automatically unlocked
- Chapter list displays updated statuses correctly
- Mastered chapters show checkmark or "Completed" badge
- Unlocked chapters are clickable
- Chapters beyond next remain locked
- If final chapter is mastered, show completion message

## Technical Notes
- Update chapter status in database after assessment completion
- PATCH /api/books/:id/chapters/:num/unlock endpoint
- Unlock logic: if chapter N is mastered, unlock chapter N+1
- Return updated chapter list to frontend
- Frontend refreshes chapter list on navigation back
- Add visual indicators for mastered vs unlocked vs locked states
- Handle edge case: final chapter has no next chapter

## Test Data
- current_chapter: 1
- next_chapter: 2
- status_after_pass: "mastered"

## Success Signals
- Mastered
- Unlocked
- Next Chapter Available
- Continue

## Failure Signals
- Error
- Failed to unlock
- locked
