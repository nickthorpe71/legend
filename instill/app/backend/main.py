from fastapi import FastAPI, UploadFile, File, HTTPException
from fastapi.middleware.cors import CORSMiddleware
from pathlib import Path
import json
import uuid
from datetime import datetime
from PyPDF2 import PdfReader
import io
import re
from typing import List, Dict
from llm_service import get_llm_service

app = FastAPI(title="Instill API", version="0.1.0")

# Configure CORS for local development
app.add_middleware(
    CORSMiddleware,
    allow_origins=["http://localhost:5173"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

# Data directory
DATA_DIR = Path("data/books")
DATA_DIR.mkdir(parents=True, exist_ok=True)

@app.get("/")
async def root():
    return {"message": "Instill API - Learning Mastery System"}

@app.get("/api/health")
async def health():
    return {"status": "ok"}

@app.post("/api/upload")
async def upload_pdf(file: UploadFile = File(...)):
    """Upload and process a PDF file"""

    # Validate file type
    if not file.filename.endswith('.pdf'):
        raise HTTPException(status_code=400, detail="File must be a PDF")

    try:
        # Read file content
        content = await file.read()

        # Extract text from PDF
        pdf_file = io.BytesIO(content)
        pdf_reader = PdfReader(pdf_file)

        # Extract text from all pages
        extracted_text = []
        for page_num, page in enumerate(pdf_reader.pages):
            page_text = page.extract_text()
            extracted_text.append({
                "page": page_num + 1,
                "text": page_text
            })

        # Generate unique book ID
        book_id = str(uuid.uuid4())

        # Save extracted data
        book_data = {
            "id": book_id,
            "filename": file.filename,
            "upload_date": datetime.now().isoformat(),
            "total_pages": len(pdf_reader.pages),
            "extracted_text": extracted_text
        }

        book_path = DATA_DIR / f"{book_id}.json"
        with open(book_path, 'w', encoding='utf-8') as f:
            json.dump(book_data, f, indent=2, ensure_ascii=False)

        return {
            "success": True,
            "book_id": book_id,
            "filename": file.filename,
            "pages": len(pdf_reader.pages),
            "message": "PDF processed successfully"
        }

    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Failed to process PDF: {str(e)}")

def detect_chapters(extracted_text: List[Dict]) -> List[Dict]:
    """Detect chapters from extracted PDF text"""
    chapters = []

    # Combine all text for pattern matching
    all_text = "\n".join([page["text"] for page in extracted_text])

    # Pattern to detect chapter headings
    # Matches: "Chapter 1", "Chapter One", "CHAPTER 1:", etc.
    chapter_pattern = re.compile(
        r'(?:^|\n)\s*(Chapter\s+(?:\d+|[IVX]+|One|Two|Three|Four|Five|Six|Seven|Eight|Nine|Ten)[:\s\-]*[^\n]*)',
        re.IGNORECASE | re.MULTILINE
    )

    matches = list(chapter_pattern.finditer(all_text))

    # If no chapters detected, create single chapter from all content
    if not matches:
        chapters.append({
            "id": 1,
            "title": "Full Text",
            "start_page": 1,
            "end_page": len(extracted_text),
            "status": "unlocked",
            "estimated_read_time": estimate_read_time(all_text)
        })
        return chapters

    # Process detected chapters
    for i, match in enumerate(matches):
        chapter_num = i + 1
        chapter_title = match.group(1).strip()

        # Find which page this chapter starts on
        char_pos = match.start()
        current_pos = 0
        start_page = 1

        for page in extracted_text:
            page_text_len = len(page["text"]) + 1  # +1 for newline
            if current_pos + page_text_len > char_pos:
                start_page = page["page"]
                break
            current_pos += page_text_len

        # Determine end page (next chapter start or end of book)
        if i < len(matches) - 1:
            next_char_pos = matches[i + 1].start()
            end_page = start_page
            current_pos = 0
            for page in extracted_text:
                page_text_len = len(page["text"]) + 1
                if current_pos + page_text_len > next_char_pos:
                    end_page = page["page"] - 1
                    break
                current_pos += page_text_len
        else:
            end_page = len(extracted_text)

        # Extract chapter text for read time estimation
        chapter_text = "\n".join([
            page["text"] for page in extracted_text
            if start_page <= page["page"] <= end_page
        ])

        chapters.append({
            "id": chapter_num,
            "title": chapter_title,
            "start_page": start_page,
            "end_page": end_page,
            "status": "unlocked" if chapter_num == 1 else "locked",
            "estimated_read_time": estimate_read_time(chapter_text)
        })

    return chapters

def estimate_read_time(text: str) -> str:
    """Estimate reading time based on word count (200 words/min)"""
    word_count = len(text.split())
    minutes = max(1, round(word_count / 200))

    if minutes < 60:
        return f"{minutes} min"
    else:
        hours = minutes // 60
        remaining_mins = minutes % 60
        if remaining_mins == 0:
            return f"{hours} hr"
        return f"{hours} hr {remaining_mins} min"

@app.get("/api/books/{book_id}/chapters")
async def get_chapters(book_id: str):
    """Get chapters for a specific book"""
    book_path = DATA_DIR / f"{book_id}.json"

    if not book_path.exists():
        raise HTTPException(status_code=404, detail="Book not found")

    try:
        with open(book_path, 'r', encoding='utf-8') as f:
            book_data = json.load(f)

        # Check if chapters already exist
        if "chapters" not in book_data:
            # Detect and add chapters
            chapters = detect_chapters(book_data["extracted_text"])
            book_data["chapters"] = chapters

            # Save updated book data
            with open(book_path, 'w', encoding='utf-8') as f:
                json.dump(book_data, f, indent=2, ensure_ascii=False)

        return {
            "book_id": book_id,
            "filename": book_data["filename"],
            "total_pages": book_data["total_pages"],
            "chapters": book_data["chapters"]
        }

    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Failed to load chapters: {str(e)}")

@app.get("/api/books/{book_id}/chapters/{chapter_num}/content")
async def get_chapter_content(book_id: str, chapter_num: int):
    """Get content for a specific chapter"""
    book_path = DATA_DIR / f"{book_id}.json"

    if not book_path.exists():
        raise HTTPException(status_code=404, detail="Book not found")

    try:
        with open(book_path, 'r', encoding='utf-8') as f:
            book_data = json.load(f)

        # Ensure chapters exist
        if "chapters" not in book_data:
            chapters = detect_chapters(book_data["extracted_text"])
            book_data["chapters"] = chapters
            with open(book_path, 'w', encoding='utf-8') as f:
                json.dump(book_data, f, indent=2, ensure_ascii=False)

        # Find the requested chapter
        chapter = next(
            (ch for ch in book_data["chapters"] if ch["id"] == chapter_num),
            None
        )

        if not chapter:
            raise HTTPException(status_code=404, detail="Chapter not found")

        # Extract chapter text from pages
        chapter_text = []
        for page in book_data["extracted_text"]:
            if chapter["start_page"] <= page["page"] <= chapter["end_page"]:
                chapter_text.append(page["text"])

        return {
            "book_id": book_id,
            "chapter": chapter,
            "content": "\n\n".join(chapter_text),
            "total_pages": book_data["total_pages"]
        }

    except HTTPException:
        raise
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Failed to load chapter content: {str(e)}")

def grade_answer_mock(question: Dict, user_answer) -> Dict:
    """Mock LLM grading (placeholder for real LLM integration)"""
    # For MVP, use simple grading logic
    # In production, this would call an LLM to evaluate the answer

    if question["type"] == "multiple_choice":
        # Check if answer matches correct index
        is_correct = user_answer == question["correct_answer"]
        return {
            "score": 100 if is_correct else 0,
            "feedback": question["explanation"] if is_correct else "Incorrect. " + question["explanation"]
        }
    else:
        # For open-ended questions, give partial credit
        answer_length = len(str(user_answer).split())
        if answer_length < 5:
            return {
                "score": 40,
                "feedback": "Your answer is too brief. Try to provide more detail and explanation."
            }
        elif answer_length < 20:
            return {
                "score": 70,
                "feedback": "Good effort! Your answer shows understanding but could be more comprehensive."
            }
        else:
            return {
                "score": 90,
                "feedback": "Excellent! Your answer demonstrates thorough understanding of the concept."
            }

def generate_mock_questions(chapter_text: str, chapter_title: str) -> List[Dict]:
    """Generate mock assessment questions (placeholder for LLM integration)"""
    # For MVP, generate simple mock questions
    # In production, this would call an LLM to generate questions from the text

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
            "question": "Which of the following best describes the key takeaway?",
            "options": [
                "Memorization is key",
                "Understanding through practice",
                "Reading once is enough",
                "Skip the details"
            ],
            "correct_answer": 1,
            "explanation": "The key takeaway emphasizes understanding through practice."
        },
        {
            "id": 3,
            "type": "multiple_choice",
            "question": "What approach does this chapter recommend?",
            "options": [
                "Quick skimming",
                "Deep comprehension and application",
                "Passive reading only",
                "Focus on titles only"
            ],
            "correct_answer": 1,
            "explanation": "The chapter recommends deep comprehension and application."
        }
    ]

    return questions

@app.post("/api/books/{book_id}/chapters/{chapter_num}/assessment/generate")
async def generate_assessment(book_id: str, chapter_num: int):
    """Generate assessment questions for a chapter"""
    book_path = DATA_DIR / f"{book_id}.json"

    if not book_path.exists():
        raise HTTPException(status_code=404, detail="Book not found")

    try:
        with open(book_path, 'r', encoding='utf-8') as f:
            book_data = json.load(f)

        # Ensure chapters exist
        if "chapters" not in book_data:
            chapters = detect_chapters(book_data["extracted_text"])
            book_data["chapters"] = chapters

        # Find the requested chapter
        chapter = next(
            (ch for ch in book_data["chapters"] if ch["id"] == chapter_num),
            None
        )

        if not chapter:
            raise HTTPException(status_code=404, detail="Chapter not found")

        # Check if already passed
        if chapter.get("mastery_achieved"):
            return {
                "already_mastered": True,
                "message": "You've already mastered this chapter!",
                "score": chapter.get("best_score", 100)
            }

        # Extract chapter text
        chapter_text = []
        for page in book_data["extracted_text"]:
            if chapter["start_page"] <= page["page"] <= chapter["end_page"]:
                chapter_text.append(page["text"])

        chapter_content = "\n\n".join(chapter_text)

        # Generate questions using LLM service (with fallback to mock)
        llm = get_llm_service()
        questions = await llm.generate_questions(chapter_content, chapter["title"], num_questions=3)

        # Create assessment record
        assessment_id = str(uuid.uuid4())
        assessment_data = {
            "id": assessment_id,
            "book_id": book_id,
            "chapter_num": chapter_num,
            "created_at": datetime.now().isoformat(),
            "questions": questions,
            "status": "pending",
            "answers": [],
            "score": None
        }

        # Save assessment data
        assessments_dir = DATA_DIR / "assessments"
        assessments_dir.mkdir(exist_ok=True)

        assessment_path = assessments_dir / f"{assessment_id}.json"
        with open(assessment_path, 'w', encoding='utf-8') as f:
            json.dump(assessment_data, f, indent=2)

        return {
            "assessment_id": assessment_id,
            "total_questions": len(questions),
            "mastery_threshold": 90,
            "message": "Assessment ready. You need 90% to pass."
        }

    except HTTPException:
        raise
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Failed to generate assessment: {str(e)}")

@app.get("/api/assessments/{assessment_id}")
async def get_assessment(assessment_id: str):
    """Get assessment questions and status"""
    assessments_dir = DATA_DIR / "assessments"
    assessment_path = assessments_dir / f"{assessment_id}.json"

    if not assessment_path.exists():
        raise HTTPException(status_code=404, detail="Assessment not found")

    try:
        with open(assessment_path, 'r', encoding='utf-8') as f:
            assessment_data = json.load(f)

        # Don't send correct answers to frontend
        questions_without_answers = [
            {
                "id": q["id"],
                "type": q["type"],
                "question": q["question"],
                "options": q.get("options", [])
            }
            for q in assessment_data["questions"]
        ]

        return {
            "id": assessment_data["id"],
            "book_id": assessment_data["book_id"],
            "chapter_num": assessment_data["chapter_num"],
            "questions": questions_without_answers,
            "total_questions": len(assessment_data["questions"]),
            "current_question": len(assessment_data["answers"]) + 1,
            "status": assessment_data["status"]
        }

    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Failed to load assessment: {str(e)}")

@app.post("/api/assessments/{assessment_id}/answers")
async def submit_answer(assessment_id: str, answer_data: dict):
    """Submit an answer to a question"""
    assessments_dir = DATA_DIR / "assessments"
    assessment_path = assessments_dir / f"{assessment_id}.json"

    if not assessment_path.exists():
        raise HTTPException(status_code=404, detail="Assessment not found")

    try:
        with open(assessment_path, 'r', encoding='utf-8') as f:
            assessment_data = json.load(f)

        question_id = answer_data.get("question_id")
        user_answer = answer_data.get("answer")

        if not question_id or user_answer is None:
            raise HTTPException(status_code=400, detail="question_id and answer are required")

        # Find the question
        question = next(
            (q for q in assessment_data["questions"] if q["id"] == question_id),
            None
        )

        if not question:
            raise HTTPException(status_code=404, detail="Question not found")

        # Grade the answer using LLM service (with fallback to mock)
        llm = get_llm_service()
        grading_result = await llm.grade_answer(question, user_answer)

        # Record answer with grading
        assessment_data["answers"].append({
            "question_id": question_id,
            "user_answer": user_answer,
            "score": grading_result["score"],
            "feedback": grading_result["feedback"],
            "submitted_at": datetime.now().isoformat()
        })

        # Check if assessment is complete
        is_complete = len(assessment_data["answers"]) >= len(assessment_data["questions"])

        if is_complete:
            assessment_data["status"] = "completed"
            # Calculate final score
            total_score = sum(ans["score"] for ans in assessment_data["answers"])
            assessment_data["score"] = round(total_score / len(assessment_data["answers"]))

        # Save updated assessment
        with open(assessment_path, 'w', encoding='utf-8') as f:
            json.dump(assessment_data, f, indent=2)

        return {
            "success": True,
            "answered": len(assessment_data["answers"]),
            "total": len(assessment_data["questions"]),
            "complete": is_complete,
            "score": grading_result["score"],
            "feedback": grading_result["feedback"]
        }

    except HTTPException:
        raise
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Failed to submit answer: {str(e)}")

@app.get("/api/assessments/{assessment_id}/results")
async def get_assessment_results(assessment_id: str):
    """Get final results for a completed assessment"""
    assessments_dir = DATA_DIR / "assessments"
    assessment_path = assessments_dir / f"{assessment_id}.json"

    if not assessment_path.exists():
        raise HTTPException(status_code=404, detail="Assessment not found")

    try:
        with open(assessment_path, 'r', encoding='utf-8') as f:
            assessment_data = json.load(f)

        if assessment_data["status"] != "completed":
            raise HTTPException(status_code=400, detail="Assessment not completed yet")

        # Calculate results
        final_score = assessment_data["score"]
        mastery_threshold = 90
        passed = final_score >= mastery_threshold

        # Identify weak concepts (questions scored < 70)
        weak_concepts = []
        for ans in assessment_data["answers"]:
            if ans["score"] < 70:
                question = next(
                    (q for q in assessment_data["questions"] if q["id"] == ans["question_id"]),
                    None
                )
                if question:
                    weak_concepts.append(question["question"])

        # Update chapter mastery status if passed
        if passed:
            book_path = DATA_DIR / f"{assessment_data['book_id']}.json"
            if book_path.exists():
                with open(book_path, 'r', encoding='utf-8') as f:
                    book_data = json.load(f)

                # Update chapter status
                for chapter in book_data.get("chapters", []):
                    if chapter["id"] == assessment_data["chapter_num"]:
                        chapter["status"] = "unlocked"
                        chapter["mastery_achieved"] = True
                        chapter["best_score"] = final_score
                        break

                # Unlock next chapter
                next_chapter_id = assessment_data["chapter_num"] + 1
                for chapter in book_data.get("chapters", []):
                    if chapter["id"] == next_chapter_id:
                        chapter["status"] = "unlocked"
                        break

                # Save updated book data
                with open(book_path, 'w', encoding='utf-8') as f:
                    json.dump(book_data, f, indent=2, ensure_ascii=False)

        return {
            "assessment_id": assessment_id,
            "book_id": assessment_data["book_id"],
            "chapter_num": assessment_data["chapter_num"],
            "final_score": final_score,
            "mastery_threshold": mastery_threshold,
            "passed": passed,
            "status": "mastered" if passed else "review_needed",
            "weak_concepts": weak_concepts,
            "total_questions": len(assessment_data["questions"]),
            "answers": assessment_data["answers"]
        }

    except HTTPException:
        raise
    except Exception as e:
        raise HTTPException(status_code=500, detail=f"Failed to get results: {str(e)}")

if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="0.0.0.0", port=8000)
