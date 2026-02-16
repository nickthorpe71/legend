import React, { useEffect, useState, useRef } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import './ReadingView.css';

interface Chapter {
  id: number;
  title: string;
  start_page: number;
  end_page: number;
  status: string;
  estimated_read_time: string;
}

interface ChapterData {
  book_id: string;
  chapter: Chapter;
  content: string;
  total_pages: number;
}

const ReadingView: React.FC = () => {
  const { bookId, chapterNum } = useParams<{ bookId: string; chapterNum: string }>();
  const navigate = useNavigate();
  const contentRef = useRef<HTMLDivElement>(null);

  const [chapterData, setChapterData] = useState<ChapterData | null>(null);
  const [error, setError] = useState<string>('');
  const [loading, setLoading] = useState<boolean>(true);
  const [readProgress, setReadProgress] = useState<number>(0);
  const [generatingAssessment, setGeneratingAssessment] = useState<boolean>(false);

  useEffect(() => {
    const fetchChapter = async () => {
      try {
        const response = await fetch(`/api/books/${bookId}/chapters/${chapterNum}/content`);
        if (!response.ok) {
          throw new Error('Failed to load chapter');
        }
        const data = await response.json();
        setChapterData(data);

        // Restore scroll position from localStorage
        const savedPosition = localStorage.getItem(`reading_${bookId}_${chapterNum}`);
        if (savedPosition && contentRef.current) {
          setTimeout(() => {
            if (contentRef.current) {
              contentRef.current.scrollTop = parseInt(savedPosition);
            }
          }, 100);
        }
      } catch (err) {
        setError(err instanceof Error ? err.message : 'Failed to load chapter');
      } finally {
        setLoading(false);
      }
    };

    if (bookId && chapterNum) {
      fetchChapter();
    }
  }, [bookId, chapterNum]);

  useEffect(() => {
    const handleScroll = () => {
      if (!contentRef.current) return;

      const { scrollTop, scrollHeight, clientHeight } = contentRef.current;
      const scrolled = scrollTop;
      const total = scrollHeight - clientHeight;
      const progress = total > 0 ? Math.round((scrolled / total) * 100) : 0;

      setReadProgress(Math.min(100, Math.max(0, progress)));

      // Save scroll position
      localStorage.setItem(`reading_${bookId}_${chapterNum}`, scrollTop.toString());
    };

    const contentElement = contentRef.current;
    if (contentElement) {
      contentElement.addEventListener('scroll', handleScroll);
      return () => contentElement.removeEventListener('scroll', handleScroll);
    }
  }, [bookId, chapterNum, chapterData]);

  const handleReadyForAssessment = async () => {
    setGeneratingAssessment(true);
    setError('');

    try {
      const response = await fetch(`/api/books/${bookId}/chapters/${chapterNum}/assessment/generate`, {
        method: 'POST',
      });

      if (!response.ok) {
        throw new Error('Failed to generate assessment');
      }

      const data = await response.json();

      if (data.already_mastered) {
        alert(data.message);
        setGeneratingAssessment(false);
        return;
      }

      // Navigate to assessment view
      navigate(`/book/${bookId}/chapter/${chapterNum}/assessment/${data.assessment_id}`);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to generate assessment');
    } finally {
      setGeneratingAssessment(false);
    }
  };

  if (loading) {
    return (
      <div className="reading-view-container">
        <div className="loading">Loading chapter...</div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="reading-view-container">
        <div className="error-message">{error}</div>
      </div>
    );
  }

  if (!chapterData) {
    return (
      <div className="reading-view-container">
        <div className="error-message">Content not found</div>
      </div>
    );
  }

  return (
    <div className="reading-view-container">
      <div className="reading-header">
        <button className="back-button" onClick={() => navigate(`/book/${bookId}/chapters`)}>
          ← Back to Chapters
        </button>
        <div className="progress-bar">
          <div className="progress-fill" style={{ width: `${readProgress}%` }}></div>
          <span className="progress-text">{readProgress}% read</span>
        </div>
      </div>

      <div className="reading-content" ref={contentRef}>
        <h1 className="chapter-title">{chapterData.chapter.title}</h1>
        <div className="chapter-meta">
          Pages {chapterData.chapter.start_page}-{chapterData.chapter.end_page} • {chapterData.chapter.estimated_read_time}
        </div>
        <div className="chapter-text">
          {chapterData.content.split('\n\n').map((paragraph, index) => (
            <p key={index}>{paragraph}</p>
          ))}
        </div>
      </div>

      <div className="reading-footer">
        <button
          className="assessment-button"
          onClick={handleReadyForAssessment}
          disabled={generatingAssessment}
        >
          {generatingAssessment ? 'Generating Assessment...' : 'Ready for Assessment →'}
        </button>
      </div>
    </div>
  );
};

export default ReadingView;
