# Feature: Progress Dashboard

## Context
Users need visibility into their learning progress, showing strengths, weaknesses, and areas needing review across all chapters.

## Requirements
- New "Progress" navigation link
- Overview of all mastered chapters
- Concept mastery breakdown (Strong/Moderate/Weak)
- Per-chapter progress visualization
- List of weak areas needing review
- Time since last practice per concept
- Overall completion percentage
- Responsive dashboard layout

## User Flow
- Step 1: User clicks "Progress" in navigation
- Step 2: Navigate to /progress page
- Step 3: See overall completion (X of Y chapters mastered)
- Step 4: View concept mastery breakdown
- Step 5: See list of concepts categorized by strength
- Step 6: Identify weak areas to review
- Step 7: Click concept to see related chapters

## Acceptance Criteria
- Given user navigates to /progress, then dashboard is displayed
- Overall progress shows chapters mastered / total chapters
- Concepts are grouped: Strong, Moderate, Weak
- Each concept shows: name, mastery level, last practiced date
- Weak concepts are highlighted for review
- Per-chapter breakdown shows individual scores
- Dashboard is responsive for mobile viewing

## Technical Notes
- Create ProgressDashboard component in React
- Add "Progress" link to navigation
- GET /api/books/:id/progress endpoint
- Backend aggregates concept mastery data
- Calculate overall completion percentage
- Group concepts by mastery level (Strong ≥85%, Moderate 70-84%, Weak <70%)
- Return last_practiced timestamp per concept
- Use charts/graphs for visualization (optional)
- Sort weak concepts by priority (oldest first)

## Test Data
- total_chapters: 10
- mastered_chapters: 4
- completion_percentage: 40
- weak_concepts: ["metacognition", "retrieval practice"]

## Success Signals
- Progress
- Dashboard
- Mastered
- concepts
- weak areas

## Failure Signals
- Error
- No data
- Failed to load
