# Feature: Spaced Recall Questions

## Context
To ensure long-term retention, users should periodically answer recall questions from earlier chapters, especially for weak or time-decayed concepts.

## Requirements
- Inject 1-3 recall questions from previous chapters
- Trigger every N chapters (e.g., every 3 chapters)
- Prioritize weak concepts (score <70%)
- Prioritize concepts not recalled recently (>7 days)
- Mix recall questions into current chapter assessment
- Clearly label recall questions (e.g., "Recall: Chapter 2")
- Update concept mastery based on recall performance
- Don't penalize chapter mastery score for recall questions

## User Flow
- Step 1: User starts assessment for Chapter 4
- Step 2: First 5-7 questions are about Chapter 4
- Step 3: Questions 8-9 are labeled "Recall: Chapter 1"
- Step 4: User answers recall questions
- Step 5: Recall performance updates concept mastery
- Step 6: Chapter 4 score is calculated only from Chapter 4 questions

## Acceptance Criteria
- Given every Nth chapter, then recall questions are injected
- Recall questions come from weak or old concepts
- Questions are clearly labeled with source chapter
- Recall performance updates concept mastery data
- Chapter pass/fail is not affected by recall questions
- Recall questions appear naturally in assessment flow
- No more than 30% of assessment is recall questions

## Technical Notes
- Check if chapter_number % 3 == 0 for recall trigger
- Query concepts with mastery="weak" or last_recall > 7 days ago
- Generate 1-3 recall questions for selected concepts
- Tag questions with is_recall=true and source_chapter
- Frontend displays recall questions with special styling
- Separate scoring: chapter_score vs recall_score
- Update concept timestamps after recall questions

## Test Data
- current_chapter: 4
- recall_from_chapter: 1
- recall_concept: "active recall"
- question_label: "Recall: Chapter 1"

## Success Signals
- Recall
- Review
- Chapter
- from earlier

## Failure Signals
- Error
- Failed to load
