# Feature: PDF Upload UI

## Context
Users need the ability to select and upload PDF files. This feature provides the file input interface and basic validation before sending to the backend.

## Requirements
- File input accepting only .pdf files
- Display selected filename after selection
- "Upload" button to submit the file
- File size validation (max 50MB)
- File type validation (PDF only)
- Clear error messages for invalid files
- Loading state while uploading

## User Flow
- Step 1: User clicks "Upload PDF" button from landing page
- Step 2: File picker opens (accepts .pdf only)
- Step 3: User selects a PDF file
- Step 4: Selected filename is displayed
- Step 5: User clicks "Upload" button
- Step 6: File is validated (size, type)
- Step 7: Upload begins (loading indicator shown)

## Acceptance Criteria
- Given a user clicks upload, then file picker opens with .pdf filter
- When a file is selected, then the filename is displayed
- When file exceeds 50MB, then error message is shown
- When file is not PDF, then error message is shown
- When upload starts, then loading indicator is visible
- Upload button is disabled until a valid file is selected

## Technical Notes
- Use HTML file input with accept=".pdf"
- Validate file size and type in browser before upload
- Create FileUpload component in React
- Use fetch or axios for file upload (FormData)
- Add loading state to prevent multiple uploads
- POST to /api/upload endpoint (to be implemented)

## Test Data
- sample_file_name: "test-document.pdf"

## Success Signals
- Select
- Upload
- Choose File
- file
- selected

## Failure Signals
- Error
- invalid
- too large
- not supported
- failed
