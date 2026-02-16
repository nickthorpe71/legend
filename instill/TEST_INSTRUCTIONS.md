# Testing Instill - Stories 1-3

## Current Status

✅ **3 stories complete:**
1. Landing page with branding
2. PDF upload UI
3. Backend PDF processing with text extraction

## Services Running

### Frontend (Port 5173)
```bash
# Already running in background
# Access at: http://localhost:5173
```

### Backend (Port 8000)
```bash
# Already running in background
# Access at: http://localhost:8000
```

## Quick Health Check

```bash
# Check frontend
curl http://localhost:5173 | grep Instill

# Check backend
curl http://localhost:8000/api/health
```

## Manual Testing (Recommended)

### Option 1: Browser Testing

1. **Open** http://localhost:5173 in your browser
2. **Click** "Upload PDF" button
3. **Select** any PDF file from your computer
4. **Click** "Upload"
5. **See** "Processing Complete!" with a book ID

### Option 2: Command Line Testing

```bash
# Create a sample PDF (or use any existing PDF)
# Then upload it:
curl -X POST http://localhost:8000/api/upload \
  -F "file=@/path/to/your/file.pdf" \
  | python3 -m json.tool

# Expected response:
# {
#   "success": true,
#   "book_id": "uuid-here",
#   "filename": "file.pdf",
#   "pages": 10,
#   "message": "PDF processed successfully"
# }
```

## Check Extracted Data

After uploading a PDF, check the extracted data:

```bash
cd /home/nickthorpe71/code/instill/app/backend
ls -la data/books/

# View extracted text from a book:
cat data/books/<book-id>.json | python3 -m json.tool | head -50
```

## Restart Services

If you need to restart:

### Restart Backend
```bash
# Kill existing
lsof -ti:8000 | xargs kill -9

# Start fresh
cd /home/nickthorpe71/code/instill/app/backend
source venv/bin/activate
python3 main.py
```

### Restart Frontend
```bash
# Kill existing
lsof -ti:5173 | xargs kill -9

# Start fresh
cd /home/nickthorpe71/code/instill/app/frontend
npm run dev
```

## API Endpoints

- `GET /` - API info
- `GET /api/health` - Health check
- `POST /api/upload` - Upload and process PDF
  - Accepts: multipart/form-data with `file` field
  - Returns: book_id, filename, pages, success status

## What's Next

**Story 4: chapter-list** - Display detected chapters from uploaded PDF

Current progress: **3/16 stories (19% complete)**
