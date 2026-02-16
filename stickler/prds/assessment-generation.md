# Feature: LLM-Based Assessment Generation

## Context
The core differentiator of Instill is high-quality, adaptive assessments. A dedicated LLM agent must generate meaningful questions that test true understanding.

## Requirements
- Dedicated LLM agent for assessment generation (not generic prompts)
- Input: chapter text, concept breakdown, prior failures
- Generate 5-10 questions per chapter
- Question types: explanation, comparison, application, misconception
- Avoid rote memorization questions
- Generate answer keys and grading criteria
- Questions should be non-trivial and thought-provoking
- Adaptive: harder questions on success, easier on repeated failure

## User Flow
- (Backend process triggered by "Ready for Assessment")
- Step 1: User triggers assessment generation
- Step 2: Backend calls dedicated assessment LLM agent
- Step 3: Agent analyzes chapter text and extracts concepts
- Step 4: Agent generates 5-10 questions with varying difficulty
- Step 5: Each question includes: text, expected answer, grading rubric
- Step 6: Questions are stored and returned to frontend

## Acceptance Criteria
- Given chapter text, then 5-10 questions are generated
- Questions test understanding, not memorization
- Question types are varied (not all "explain X")
- Each question has grading criteria
- Questions are associated with specific concepts
- Generation completes within 15 seconds
- Questions are stored for potential reuse/adaptation

## Technical Notes
- Create dedicated LLM assessment agent (separate system prompt)
- Use Claude 3.5 Sonnet or similar high-quality model
- System prompt emphasizes: depth over breadth, reasoning over recall
- Provide chapter text and previously failed concepts
- LLM returns structured JSON: [{question, concepts[], expected_answer, rubric}]
- Store questions in assessments table
- Consider caching common concepts for consistency
- Add retry logic if LLM fails or returns malformed data

## Test Data
- chapter_text: "Chapter 1: Introduction to Learning Mastery..."
- num_questions: 7
- question_types: ["explanation", "application", "comparison"]

## Success Signals
- Generated
- questions
- Assessment ready

## Failure Signals
- Error
- Failed to generate
- timeout
- invalid response
