# Feature: Chapter List Display

## Context
After PDF processing, users need to see the detected chapters. The list shows progression state (locked/unlocked) and allows navigation to reading view.

## Requirements
- Display all detected chapters in order
- Show chapter titles
- Show estimated read time per chapter
- Indicate which chapters are locked/unlocked
- Chapter 1 is unlocked by default
- Visual distinction between locked and unlocked chapters
- Click unlocked chapter to navigate to reading view
- Clicking locked chapter shows informational message

## User Flow
- Step 1: PDF processing completes
- Step 2: User is navigated to /book/:id/chapters
- Step 3: User sees list of all chapters
- Step 4: Chapter 1 is unlocked (clickable)
- Step 5: All other chapters show lock icon
- Step 6: User clicks Chapter 1
- Step 7: Navigate to reading view for Chapter 1

## Acceptance Criteria
- Given a processed book, then all chapters are displayed in order
- Chapter 1 is unlocked, all others are locked
- Locked chapters show lock icon and are not clickable
- Unlocked chapters are clickable and navigate to reading view
- Each chapter shows title and estimated read time
- When clicking locked chapter, then message explains progression

## Technical Notes
- Create ChapterList component in React
- GET /api/books/:id/chapters endpoint
- Backend detects chapters from extracted text
- Chapter detection: look for "Chapter", headings, page breaks
- Store chapter metadata (title, start_pos, end_pos, status)
- Return chapter list with unlock status
- Use React Router for navigation

## Test Data
- book_id: "test-book-123"
- chapter_title: "Chapter 1: Introduction"

## Success Signals
- Chapter
- Locked
- Unlocked
- Start Reading
- chapters

## Failure Signals
- Error
- No chapters found
- Failed to load
