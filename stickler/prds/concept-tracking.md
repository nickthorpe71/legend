# Feature: Concept-Level Mastery Tracking

## Context
Beyond chapter pass/fail, the system must track mastery of individual concepts to enable spaced recall and personalized summaries.

## Requirements
- Identify concepts in each question
- Track score per concept (not just per question)
- Categorize concepts as Strong/Moderate/Weak
- Store last successful recall timestamp per concept
- Count failures per concept
- Associate concepts with chapters
- Persist mastery data across sessions

## User Flow
- (Background process during assessment)
- Step 1: User answers question
- Step 2: Question is tagged with concepts (e.g., "spaced repetition", "active recall")
- Step 3: Score is recorded for each concept
- Step 4: Concept mastery level is updated
- Step 5: Data is stored for future recall questions

## Acceptance Criteria
- Each question is associated with 1-3 concepts
- Concept scores are tracked separately from question scores
- Mastery levels: Strong (≥85%), Moderate (70-84%), Weak (<70%)
- Last recall timestamp is updated on correct answers
- Failure count increments on scores <70%
- All concept data is persisted to database
- Concepts can be queried for spaced recall selection

## Technical Notes
- Add concepts field to questions in database
- LLM tags questions with concepts during generation
- Store concept_scores table: concept, chapter, score, timestamp
- Calculate mastery level from average score
- Track successful_recall_count and failure_count
- Index concepts for efficient querying
- Create GET /api/books/:id/concepts endpoint
- Use concepts to generate personalized feedback

## Test Data
- concept_name: "spaced repetition"
- question_score: 85
- mastery_level: "strong"
- last_recall: "2024-01-12T10:30:00Z"

## Success Signals
- (Internal tracking, no direct UI signals)

## Failure Signals
- (None - background process)
