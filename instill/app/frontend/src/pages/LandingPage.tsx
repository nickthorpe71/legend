import React from 'react';
import { useNavigate } from 'react-router-dom';
import './LandingPage.css';

const LandingPage: React.FC = () => {
  const navigate = useNavigate();

  const handleUploadClick = () => {
    navigate('/upload');
  };

  return (
    <div className="landing-page">
      <main className="landing-content">
        <h1 className="landing-title">Instill</h1>
        <p className="landing-tagline">
          Transform passive reading into verified understanding through mastery-based learning
        </p>
        <button className="upload-button" onClick={handleUploadClick}>
          Upload PDF
        </button>
      </main>
    </div>
  );
};

export default LandingPage;
