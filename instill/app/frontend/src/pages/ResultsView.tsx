import React, { useEffect, useState } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import './ResultsView.css';

interface ResultsData {
  assessment_id: string;
  book_id: string;
  chapter_num: number;
  final_score: number;
  mastery_threshold: number;
  passed: boolean;
  status: string;
  weak_concepts: string[];
  total_questions: number;
}

const ResultsView: React.FC = () => {
  const { bookId, chapterNum, assessmentId } = useParams<{
    bookId: string;
    chapterNum: string;
    assessmentId: string;
  }>();
  const navigate = useNavigate();

  const [results, setResults] = useState<ResultsData | null>(null);
  const [error, setError] = useState<string>('');
  const [loading, setLoading] = useState<boolean>(true);

  useEffect(() => {
    const fetchResults = async () => {
      try {
        const response = await fetch(`/api/assessments/${assessmentId}/results`);
        if (!response.ok) {
          throw new Error('Failed to load results');
        }
        const data = await response.json();
        setResults(data);
      } catch (err) {
        setError(err instanceof Error ? err.message : 'Failed to load results');
      } finally {
        setLoading(false);
      }
    };

    if (assessmentId) {
      fetchResults();
    }
  }, [assessmentId]);

  const handleReturnToChapters = () => {
    navigate(`/book/${bookId}/chapters`);
  };

  const handleRetry = () => {
    navigate(`/book/${bookId}/chapter/${chapterNum}`);
  };

  if (loading) {
    return (
      <div className="results-container">
        <div className="loading">Loading results...</div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="results-container">
        <div className="error-message">{error}</div>
      </div>
    );
  }

  if (!results) {
    return (
      <div className="results-container">
        <div className="error-message">Results not found</div>
      </div>
    );
  }

  return (
    <div className="results-container">
      <div className="results-card">
        <div className={`results-header ${results.passed ? 'passed' : 'failed'}`}>
          <h1>{results.passed ? '🎉 Chapter Mastered!' : '📚 Review Needed'}</h1>
          <div className="score-display">
            <div className="score-number">{results.final_score}%</div>
            <div className="score-label">
              Mastery Threshold: {results.mastery_threshold}%
            </div>
          </div>
        </div>

        <div className="results-content">
          {results.passed ? (
            <div className="success-message">
              <p>
                Congratulations! You've demonstrated mastery of this chapter.
                The next chapter is now unlocked.
              </p>
            </div>
          ) : (
            <div className="review-message">
              <p>
                You scored {results.final_score}% on this assessment. You need{' '}
                {results.mastery_threshold}% to unlock the next chapter.
              </p>
              {results.weak_concepts.length > 0 && (
                <div className="weak-concepts">
                  <h3>Areas to Review:</h3>
                  <ul>
                    {results.weak_concepts.map((concept, index) => (
                      <li key={index}>{concept}</li>
                    ))}
                  </ul>
                </div>
              )}
            </div>
          )}

          <div className="results-stats">
            <div className="stat">
              <div className="stat-label">Questions Answered</div>
              <div className="stat-value">{results.total_questions}</div>
            </div>
            <div className="stat">
              <div className="stat-label">Final Score</div>
              <div className="stat-value">{results.final_score}%</div>
            </div>
          </div>
        </div>

        <div className="results-actions">
          {results.passed ? (
            <button className="primary-button" onClick={handleReturnToChapters}>
              Continue to Next Chapter →
            </button>
          ) : (
            <>
              <button className="secondary-button" onClick={handleReturnToChapters}>
                Return to Chapters
              </button>
              <button className="primary-button" onClick={handleRetry}>
                Review & Retry Assessment
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
};

export default ResultsView;
