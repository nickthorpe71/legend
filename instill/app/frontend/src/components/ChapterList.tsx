import React, { useEffect, useState } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import './ChapterList.css';

interface Chapter {
  id: number;
  title: string;
  start_page: number;
  end_page: number;
  status: 'locked' | 'unlocked';
  estimated_read_time: string;
}

interface BookData {
  book_id: string;
  filename: string;
  total_pages: number;
  chapters: Chapter[];
}

const ChapterList: React.FC = () => {
  const { bookId } = useParams<{ bookId: string }>();
  const navigate = useNavigate();
  const [bookData, setBookData] = useState<BookData | null>(null);
  const [error, setError] = useState<string>('');
  const [loading, setLoading] = useState<boolean>(true);

  useEffect(() => {
    const fetchChapters = async () => {
      try {
        const response = await fetch(`/api/books/${bookId}/chapters`);
        if (!response.ok) {
          throw new Error('Failed to load chapters');
        }
        const data = await response.json();
        setBookData(data);
      } catch (err) {
        setError(err instanceof Error ? err.message : 'Failed to load chapters');
      } finally {
        setLoading(false);
      }
    };

    if (bookId) {
      fetchChapters();
    }
  }, [bookId]);

  const handleChapterClick = (chapter: Chapter) => {
    if (chapter.status === 'unlocked') {
      navigate(`/book/${bookId}/chapter/${chapter.id}`);
    } else {
      alert('This chapter is locked. Complete previous chapters to unlock it.');
    }
  };

  if (loading) {
    return (
      <div className="chapter-list-container">
        <div className="loading">Loading chapters...</div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="chapter-list-container">
        <div className="error-message">{error}</div>
      </div>
    );
  }

  if (!bookData) {
    return (
      <div className="chapter-list-container">
        <div className="error-message">No chapters found</div>
      </div>
    );
  }

  return (
    <div className="chapter-list-container">
      <div className="chapter-list-header">
        <h1>{bookData.filename}</h1>
        <p className="book-info">{bookData.total_pages} pages • {bookData.chapters.length} chapters</p>
      </div>

      <div className="chapters">
        {bookData.chapters.map((chapter) => (
          <div
            key={chapter.id}
            className={`chapter-card ${chapter.status}`}
            onClick={() => handleChapterClick(chapter)}
          >
            <div className="chapter-header">
              <h3 className="chapter-title">{chapter.title}</h3>
              <span className={`chapter-status ${chapter.status}`}>
                {chapter.status === 'unlocked' ? '🔓 Unlocked' : '🔒 Locked'}
              </span>
            </div>
            <div className="chapter-details">
              <span className="page-range">
                Pages {chapter.start_page}-{chapter.end_page}
              </span>
              <span className="read-time">{chapter.estimated_read_time}</span>
            </div>
            {chapter.status === 'unlocked' && (
              <button className="start-reading-btn">Start Reading</button>
            )}
          </div>
        ))}
      </div>
    </div>
  );
};

export default ChapterList;
