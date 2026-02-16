import React, { useState, ChangeEvent } from 'react';
import { useNavigate } from 'react-router-dom';
import './FileUpload.css';

const FileUpload: React.FC = () => {
  const navigate = useNavigate();
  const [selectedFile, setSelectedFile] = useState<File | null>(null);
  const [error, setError] = useState<string>('');
  const [isUploading, setIsUploading] = useState(false);
  const [uploadSuccess, setUploadSuccess] = useState(false);
  const [bookId, setBookId] = useState<string>('');

  const MAX_FILE_SIZE = 50 * 1024 * 1024; // 50MB in bytes

  const handleFileSelect = (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    setError('');

    if (!file) {
      setSelectedFile(null);
      return;
    }

    // Validate file type
    if (file.type !== 'application/pdf') {
      setError('Please select a PDF file');
      setSelectedFile(null);
      return;
    }

    // Validate file size
    if (file.size > MAX_FILE_SIZE) {
      setError('File is too large. Maximum size is 50MB');
      setSelectedFile(null);
      return;
    }

    setSelectedFile(file);
  };

  const handleUpload = async () => {
    if (!selectedFile) return;

    setIsUploading(true);
    setError('');

    try {
      const formData = new FormData();
      formData.append('file', selectedFile);

      const response = await fetch('/api/upload', {
        method: 'POST',
        body: formData,
      });

      if (!response.ok) {
        throw new Error('Upload failed');
      }

      const data = await response.json();
      console.log('Upload successful:', data);

      setUploadSuccess(true);
      setBookId(data.book_id);

      // Navigate to chapter list
      setTimeout(() => {
        navigate(`/book/${data.book_id}/chapters`);
      }, 1500);
    } catch (err) {
      setError('Upload failed. Please try again.');
    } finally {
      setIsUploading(false);
    }
  };

  return (
    <div className="file-upload">
      <div className="file-upload-content">
        <h2>Upload Your PDF</h2>
        <p className="upload-description">
          Select a PDF file to begin your learning journey
        </p>

        <div className="file-input-container">
          <label htmlFor="file-input" className="file-input-label">
            Choose File
          </label>
          <input
            id="file-input"
            type="file"
            accept=".pdf"
            onChange={handleFileSelect}
            className="file-input"
          />
        </div>

        {selectedFile && (
          <div className="file-selected">
            <p className="selected-filename">
              Selected: {selectedFile.name}
            </p>
            <button
              onClick={handleUpload}
              disabled={isUploading}
              className="upload-button"
            >
              {isUploading ? 'Uploading...' : 'Upload'}
            </button>
          </div>
        )}

        {error && <p className="error-message">{error}</p>}

        {uploadSuccess && (
          <div className="success-message">
            <h3>Processing Complete!</h3>
            <p>Your PDF has been successfully processed.</p>
            <p className="book-id">Book ID: {bookId}</p>
          </div>
        )}
      </div>
    </div>
  );
};

export default FileUpload;
