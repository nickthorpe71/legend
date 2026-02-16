"""
LLM Service for Assessment Generation and Grading

Supports multiple backends:
- Ollama (local, free, private)
- OpenAI API (cloud, paid)
- Anthropic Claude API (cloud, paid)
- Mock (for testing/development)
"""

import os
import json
from typing import List, Dict
import httpx


class LLMService:
    def __init__(self, backend: str = "ollama"):
        """
        Initialize LLM service with specified backend

        Args:
            backend: "ollama", "openai", "anthropic", or "mock"
        """
        self.backend = backend
        self.ollama_url = os.getenv("OLLAMA_URL", "http://localhost:11434")
        self.ollama_model = os.getenv("OLLAMA_MODEL", "llama3.2")
        self.openai_key = os.getenv("OPENAI_API_KEY")
        self.anthropic_key = os.getenv("ANTHROPIC_API_KEY")

    async def generate_questions(self, chapter_text: str, chapter_title: str, num_questions: int = 3) -> List[Dict]:
        """Generate assessment questions from chapter text"""

        if self.backend == "ollama":
            return await self._generate_questions_ollama(chapter_text, chapter_title, num_questions)
        elif self.backend == "mock":
            return self._generate_questions_mock(chapter_text, chapter_title, num_questions)
        else:
            raise NotImplementedError(f"Backend {self.backend} not implemented yet")

    async def grade_answer(self, question: Dict, user_answer: str) -> Dict:
        """Grade a user's answer to a question"""

        if self.backend == "ollama":
            return await self._grade_answer_ollama(question, user_answer)
        elif self.backend == "mock":
            return self._grade_answer_mock(question, user_answer)
        else:
            raise NotImplementedError(f"Backend {self.backend} not implemented yet")

    async def _generate_questions_ollama(self, chapter_text: str, chapter_title: str, num_questions: int) -> List[Dict]:
        """Generate questions using Ollama local LLM"""

        # Truncate text if too long (Ollama context limits)
        max_chars = 8000
        if len(chapter_text) > max_chars:
            chapter_text = chapter_text[:max_chars] + "..."

        prompt = f"""You are an expert educational assessment designer. Generate {num_questions} high-quality multiple choice questions to test deep understanding of the following chapter.

Chapter Title: {chapter_title}

Chapter Content:
{chapter_text}

Requirements:
- Focus on conceptual understanding, not rote memorization
- Test application and reasoning, not just recall
- Each question should have 4 options
- Include the correct answer index (0-3)
- Provide a brief explanation for the correct answer

Return ONLY valid JSON in this exact format (no markdown, no extra text):
{{
  "questions": [
    {{
      "id": 1,
      "type": "multiple_choice",
      "question": "Question text here?",
      "options": ["Option A", "Option B", "Option C", "Option D"],
      "correct_answer": 0,
      "explanation": "Explanation here"
    }}
  ]
}}"""

        try:
            async with httpx.AsyncClient(timeout=60.0) as client:
                response = await client.post(
                    f"{self.ollama_url}/api/generate",
                    json={
                        "model": self.ollama_model,
                        "prompt": prompt,
                        "stream": False,
                        "format": "json"
                    }
                )

                if response.status_code != 200:
                    print(f"Ollama error: {response.status_code} - {response.text}")
                    return self._generate_questions_mock(chapter_text, chapter_title, num_questions)

                result = response.json()
                generated_text = result.get("response", "")

                # Parse the JSON response
                try:
                    parsed = json.loads(generated_text)
                    questions = parsed.get("questions", [])

                    # Validate and return questions
                    if questions and len(questions) > 0:
                        return questions
                    else:
                        print("No questions in LLM response, falling back to mock")
                        return self._generate_questions_mock(chapter_text, chapter_title, num_questions)

                except json.JSONDecodeError as e:
                    print(f"Failed to parse LLM JSON: {e}")
                    print(f"Generated text: {generated_text[:500]}")
                    return self._generate_questions_mock(chapter_text, chapter_title, num_questions)

        except Exception as e:
            print(f"Ollama connection error: {e}")
            print("Falling back to mock questions")
            return self._generate_questions_mock(chapter_text, chapter_title, num_questions)

    async def _grade_answer_ollama(self, question: Dict, user_answer) -> Dict:
        """Grade answer using Ollama local LLM"""

        if question["type"] == "multiple_choice":
            # Direct comparison for multiple choice
            is_correct = user_answer == question["correct_answer"]
            return {
                "score": 100 if is_correct else 0,
                "feedback": question["explanation"] if is_correct else f"Incorrect. {question['explanation']}"
            }

        # For open-ended questions, use LLM to grade
        prompt = f"""You are an expert educational evaluator. Grade the following student answer.

Question: {question['question']}

Student Answer: {user_answer}

Expected understanding level: The student should demonstrate comprehension of the core concepts.

Return ONLY valid JSON in this exact format:
{{
  "score": 85,
  "feedback": "Your feedback here explaining what was good and what could be improved"
}}

Score should be 0-100. Give partial credit for partially correct answers."""

        try:
            async with httpx.AsyncClient(timeout=30.0) as client:
                response = await client.post(
                    f"{self.ollama_url}/api/generate",
                    json={
                        "model": self.ollama_model,
                        "prompt": prompt,
                        "stream": False,
                        "format": "json"
                    }
                )

                if response.status_code != 200:
                    return self._grade_answer_mock(question, user_answer)

                result = response.json()
                generated_text = result.get("response", "")

                try:
                    parsed = json.loads(generated_text)
                    return {
                        "score": parsed.get("score", 70),
                        "feedback": parsed.get("feedback", "Good effort!")
                    }
                except:
                    return self._grade_answer_mock(question, user_answer)

        except Exception as e:
            print(f"Grading error: {e}")
            return self._grade_answer_mock(question, user_answer)

    def _generate_questions_mock(self, chapter_text: str, chapter_title: str, num_questions: int) -> List[Dict]:
        """Fallback mock question generation"""
        questions = [
            {
                "id": 1,
                "type": "multiple_choice",
                "question": f"What is the main topic discussed in {chapter_title}?",
                "options": [
                    "Understanding the core concepts",
                    "Historical background",
                    "Future predictions",
                    "Unrelated topic"
                ],
                "correct_answer": 0,
                "explanation": "This chapter focuses on understanding the core concepts."
            },
            {
                "id": 2,
                "type": "multiple_choice",
                "question": "Which approach does this chapter recommend?",
                "options": [
                    "Quick skimming",
                    "Deep comprehension and application",
                    "Passive reading only",
                    "Focus on titles only"
                ],
                "correct_answer": 1,
                "explanation": "The chapter recommends deep comprehension and application."
            },
            {
                "id": 3,
                "type": "multiple_choice",
                "question": "What is the key takeaway from this chapter?",
                "options": [
                    "Memorization is sufficient",
                    "Understanding through practice and application",
                    "Reading once is enough",
                    "Skip the difficult parts"
                ],
                "correct_answer": 1,
                "explanation": "The key takeaway emphasizes understanding through practice and application."
            }
        ]

        return questions[:num_questions]

    def _grade_answer_mock(self, question: Dict, user_answer) -> Dict:
        """Fallback mock grading"""
        if question["type"] == "multiple_choice":
            is_correct = user_answer == question.get("correct_answer")
            return {
                "score": 100 if is_correct else 0,
                "feedback": question.get("explanation", "Correct!") if is_correct else f"Incorrect. {question.get('explanation', '')}"
            }
        else:
            # Simple length-based grading for open-ended
            answer_length = len(str(user_answer).split())
            if answer_length < 5:
                return {"score": 40, "feedback": "Your answer is too brief. Try to provide more detail."}
            elif answer_length < 20:
                return {"score": 70, "feedback": "Good effort! Your answer shows understanding but could be more comprehensive."}
            else:
                return {"score": 90, "feedback": "Excellent! Your answer demonstrates thorough understanding."}


# Singleton instance
_llm_service = None

def get_llm_service() -> LLMService:
    """Get or create LLM service singleton"""
    global _llm_service
    if _llm_service is None:
        # Check if Ollama is available, fallback to mock
        backend = os.getenv("LLM_BACKEND", "ollama")
        _llm_service = LLMService(backend=backend)
    return _llm_service
