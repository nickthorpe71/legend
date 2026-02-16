import React from 'react';
import { BrowserRouter as Router, Routes, Route } from 'react-router-dom';
import LandingPage from './pages/LandingPage';
import FileUpload from './components/FileUpload';
import ChapterList from './components/ChapterList';
import ReadingView from './pages/ReadingView';
import AssessmentView from './pages/AssessmentView';
import ResultsView from './pages/ResultsView';

function App() {
  return (
    <Router>
      <div className="app">
        <Routes>
          <Route path="/" element={<LandingPage />} />
          <Route path="/upload" element={<FileUpload />} />
          <Route path="/book/:bookId/chapters" element={<ChapterList />} />
          <Route path="/book/:bookId/chapter/:chapterNum" element={<ReadingView />} />
          <Route
            path="/book/:bookId/chapter/:chapterNum/assessment/:assessmentId"
            element={<AssessmentView />}
          />
          <Route
            path="/book/:bookId/chapter/:chapterNum/assessment/:assessmentId/results"
            element={<ResultsView />}
          />
        </Routes>
      </div>
    </Router>
  );
}

export default App;
