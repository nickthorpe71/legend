# Feature: Reading View

## Context
Users need to read chapter content with clear formatting and track their progress. The reading view is where actual learning happens before assessment.

## Requirements
- Display full chapter text with readable formatting
- Show chapter title at top
- Progress indicator showing % of chapter read
- "Ready for Assessment" button at bottom
- Smooth scrolling experience
- Preserve reading position on page refresh
- Responsive text sizing for mobile

## User Flow
- Step 1: User clicks unlocked chapter from chapter list
- Step 2: Navigate to /book/:id/chapter/:chapterNum
- Step 3: Chapter title and content displayed
- Step 4: User scrolls through chapter content
- Step 5: Progress indicator updates as user scrolls
- Step 6: User reaches end of chapter
- Step 7: User clicks "Ready for Assessment" button

## Acceptance Criteria
- Given an unlocked chapter, then full text is displayed
- Chapter title is shown prominently at top
- Progress indicator shows 0-100% based on scroll position
- "Ready for Assessment" button is visible
- Text is formatted for readability (line height, font size)
- Scrolling is smooth and responsive
- Button is sticky/fixed for easy access

## Technical Notes
- Create ReadingView component in React
- GET /api/books/:id/chapters/:num/content endpoint
- Use scroll event listener to calculate progress
- Store scroll position in localStorage
- Format text with proper line breaks and paragraphs
- Consider using markdown rendering if text contains formatting
- Button triggers navigation to assessment

## Test Data
- book_id: "test-book-123"
- chapter_num: 1
- chapter_title: "Introduction to Learning Mastery"

## Success Signals
- Ready for Assessment
- Continue
- progress
- chapter

## Failure Signals
- Error
- Failed to load
- Content not found
