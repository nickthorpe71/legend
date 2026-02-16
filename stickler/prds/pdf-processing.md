# Feature: PDF Processing

## Context
After a PDF is uploaded, the backend must extract text and prepare it for chapter detection. The user needs feedback that processing is happening.

## Requirements
- Accept PDF file upload via POST /api/upload
- Extract text from PDF using PyPDF2 or pdfplumber
- Store extracted text (file system or database)
- Return processing status to frontend
- Display "Processing..." message to user
- Show "Complete" when extraction finishes
- Handle extraction errors gracefully

## User Flow
- Step 1: User uploads valid PDF file
- Step 2: Backend receives file and starts processing
- Step 3: User sees "Processing..." message
- Step 4: Backend extracts text from PDF
- Step 5: Text is stored with unique ID
- Step 6: User sees "Processing complete" message
- Step 7: UI navigates to chapter list (or next step)

## Acceptance Criteria
- Given a PDF is uploaded, then backend extracts all text content
- When processing starts, then user sees "Processing" indicator
- When processing completes, then user sees "Complete" message
- When PDF is unreadable, then clear error message is shown
- Extracted text is stored and associated with book ID
- Processing completes within 30 seconds for typical books

## Technical Notes
- Create /api/upload endpoint (POST)
- Use python-multipart for file handling
- Install PyPDF2 or pdfplumber for text extraction
- Store extracted text in books/ directory as JSON
- Return book_id to frontend for tracking
- Consider chunking for very large PDFs
- Add error handling for corrupted PDFs

## Test Data
- None (uses uploaded PDF)

## Success Signals
- Processing
- Complete
- extracted
- Success

## Failure Signals
- Error
- Failed
- corrupted
- unreadable
- invalid PDF
