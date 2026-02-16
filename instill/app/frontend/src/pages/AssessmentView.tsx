import React, { useEffect, useState, useRef } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import './AssessmentView.css';

interface Question {
  id: number;
  type: string;
  question: string;
  options: string[];
}

interface AssessmentData {
  id: string;
  book_id: string;
  chapter_num: number;
  questions: Question[];
  total_questions: number;
  current_question: number;
  status: string;
}

const AssessmentView: React.FC = () => {
  const { bookId, chapterNum, assessmentId } = useParams<{
    bookId: string;
    chapterNum: string;
    assessmentId: string;
  }>();
  const navigate = useNavigate();
  const answerRef = useRef<HTMLTextAreaElement>(null);

  const [assessmentData, setAssessmentData] = useState<AssessmentData | null>(null);
  const [error, setError] = useState<string>('');
  const [loading, setLoading] = useState<boolean>(true);
  const [currentQuestionIndex, setCurrentQuestionIndex] = useState<number>(0);
  const [answer, setAnswer] = useState<string>('');
  const [submitting, setSubmitting] = useState<boolean>(false);
  const [selectedOption, setSelectedOption] = useState<number | null>(null);
  const [showFeedback, setShowFeedback] = useState<boolean>(false);
  const [feedback, setFeedback] = useState<{ score: number; message: string } | null>(null);

  useEffect(() => {
    const fetchAssessment = async () => {
      try {
        const response = await fetch(`/api/assessments/${assessmentId}`);
        if (!response.ok) {
          throw new Error('Failed to load assessment');
        }
        const data = await response.json();
        setAssessmentData(data);
        setCurrentQuestionIndex(data.current_question - 1);
      } catch (err) {
        setError(err instanceof Error ? err.message : 'Failed to load assessment');
      } finally {
        setLoading(false);
      }
    };

    if (assessmentId) {
      fetchAssessment();
    }
  }, [assessmentId]);

  useEffect(() => {
    // Auto-focus answer input
    if (answerRef.current) {
      answerRef.current.focus();
    }
  }, [currentQuestionIndex]);

  const handleSubmitAnswer = async () => {
    if (!assessmentData) return;

    const currentQuestion = assessmentData.questions[currentQuestionIndex];
    const userAnswer = currentQuestion.type === 'multiple_choice' ? selectedOption : answer;

    if (userAnswer === null || userAnswer === '') return;

    setSubmitting(true);
    setError('');

    try {
      const response = await fetch(`/api/assessments/${assessmentId}/answers`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          question_id: currentQuestion.id,
          answer: userAnswer,
        }),
      });

      if (!response.ok) {
        throw new Error('Failed to submit answer');
      }

      const result = await response.json();

      // Show feedback
      setFeedback({
        score: result.score,
        message: result.feedback
      });
      setShowFeedback(true);

      // Auto-advance after showing feedback
      setTimeout(() => {
        setShowFeedback(false);
        setFeedback(null);

        if (result.complete) {
          // Navigate to grading/results page
          navigate(`/book/${bookId}/chapter/${chapterNum}/assessment/${assessmentId}/results`);
        } else {
          // Move to next question
          setCurrentQuestionIndex(currentQuestionIndex + 1);
          setAnswer('');
          setSelectedOption(null);
        }
      }, 3000); // 3 second delay to read feedback
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to submit answer');
    } finally {
      setSubmitting(false);
    }
  };

  if (loading) {
    return (
      <div className="assessment-container">
        <div className="loading">Loading assessment...</div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="assessment-container">
        <div className="error-message">{error}</div>
      </div>
    );
  }

  if (!assessmentData) {
    return (
      <div className="assessment-container">
        <div className="error-message">Assessment not found</div>
      </div>
    );
  }

  const currentQuestion = assessmentData.questions[currentQuestionIndex];
  const isMultipleChoice = currentQuestion?.type === 'multiple_choice';
  const canSubmit = isMultipleChoice
    ? selectedOption !== null
    : answer.trim().length > 0;

  return (
    <div className="assessment-container">
      <div className="assessment-header">
        <h1>Assessment</h1>
        <div className="question-counter">
          Question {currentQuestionIndex + 1} of {assessmentData.total_questions}
        </div>
      </div>

      <div className="assessment-content">
        <div className="question-card">
          <h2 className="question-text">{currentQuestion.question}</h2>

          {isMultipleChoice ? (
            <div className="options-container">
              {currentQuestion.options.map((option, index) => (
                <div
                  key={index}
                  className={`option ${selectedOption === index ? 'selected' : ''}`}
                  onClick={() => setSelectedOption(index)}
                >
                  <div className="option-radio">
                    {selectedOption === index ? '●' : '○'}
                  </div>
                  <div className="option-text">{option}</div>
                </div>
              ))}
            </div>
          ) : (
            <textarea
              ref={answerRef}
              className="answer-input"
              placeholder="Type your answer here..."
              value={answer}
              onChange={(e) => setAnswer(e.target.value)}
              disabled={submitting}
            />
          )}

          <button
            className="submit-button"
            onClick={handleSubmitAnswer}
            disabled={!canSubmit || submitting || showFeedback}
          >
            {submitting ? 'Submitting...' : 'Submit Answer'}
          </button>
        </div>

        {showFeedback && feedback && (
          <div className="feedback-overlay">
            <div className="feedback-card">
              <div className={`feedback-score ${feedback.score >= 70 ? 'good' : 'needs-work'}`}>
                Score: {feedback.score}/100
              </div>
              <div className="feedback-message">{feedback.message}</div>
              <div className="feedback-next">Next question loading...</div>
            </div>
          </div>
        )}
      </div>

      <div className="assessment-info">
        <p>Mastery Threshold: 90%</p>
      </div>
    </div>
  );
};

export default AssessmentView;
