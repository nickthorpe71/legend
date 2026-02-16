#!/bin/bash

echo "=== Testing Instill Full Stack ==="
echo ""

# Check frontend
echo "1. Frontend (React):"
curl -s http://localhost:5173 | grep -q "Instill" && echo "   ✅ Running on http://localhost:5173" || echo "   ❌ Not responding"
echo ""

# Check backend
echo "2. Backend (FastAPI):"
curl -s http://localhost:8000/api/health | grep -q "ok" && echo "   ✅ Running on http://localhost:8000" || echo "   ❌ Not responding"
echo ""

# Create a test PDF
echo "3. Creating test PDF..."
cat > /tmp/test.html <<EOF
<!DOCTYPE html>
<html>
<head><title>Test Book</title></head>
<body>
<h1>Chapter 1: Introduction</h1>
<p>This is a test book for Instill. Learning mastery is about transforming passive reading into verified understanding.</p>

<h1>Chapter 2: Key Concepts</h1>
<p>Spaced repetition and active recall are fundamental to long-term retention.</p>
</body>
</html>
EOF

# Convert HTML to PDF if wkhtmltopdf is available
if command -v wkhtmltopdf &> /dev/null; then
    wkhtmltopdf -q /tmp/test.html /tmp/test-book.pdf 2>/dev/null
    echo "   ✅ Test PDF created at /tmp/test-book.pdf"
else
    # Fallback: create a minimal PDF using Python
    python3 << 'PYTHON'
from PyPDF2 import PdfWriter
import io

# Create a simple PDF
writer = PdfWriter()
# Note: Creating a minimal valid PDF for testing
with open('/tmp/test-book.pdf', 'wb') as f:
    # Write minimal PDF structure
    f.write(b'%PDF-1.4\n')
    f.write(b'1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n')
    f.write(b'2 0 obj<</Type/Pages/Count 1/Kids[3 0 R]>>endobj\n')
    f.write(b'3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]>>endobj\n')
    f.write(b'xref\n0 4\n0000000000 65535 f\n0000000009 00000 n\n0000000056 00000 n\n0000000115 00000 n\n')
    f.write(b'trailer<</Size 4/Root 1 0 R>>\nstartxref\n190\n%%EOF\n')

print("   ✅ Test PDF created at /tmp/test-book.pdf")
PYTHON
fi

echo ""

# Test upload endpoint
echo "4. Testing PDF upload:"
RESPONSE=$(curl -s -X POST http://localhost:8000/api/upload \
  -F "file=@/tmp/test-book.pdf" \
  -H "Content-Type: multipart/form-data")

if echo "$RESPONSE" | grep -q "success"; then
    echo "   ✅ Upload successful!"
    echo "$RESPONSE" | python3 -m json.tool | grep -E "(success|book_id|filename|pages|message)"

    # Extract book_id
    BOOK_ID=$(echo "$RESPONSE" | python3 -c "import sys, json; print(json.load(sys.stdin).get('book_id', ''))")

    if [ -n "$BOOK_ID" ]; then
        echo ""
        echo "5. Checking stored data:"
        if [ -f "app/backend/data/books/${BOOK_ID}.json" ]; then
            echo "   ✅ Book data saved at app/backend/data/books/${BOOK_ID}.json"
            echo "   Pages extracted:" $(cat "app/backend/data/books/${BOOK_ID}.json" | python3 -c "import sys, json; print(json.load(sys.stdin)['total_pages'])")
        else
            echo "   ❌ Book data not found"
        fi
    fi
else
    echo "   ❌ Upload failed"
    echo "$RESPONSE"
fi

echo ""
echo "=== Manual Testing ==="
echo "Open http://localhost:5173 in your browser to test:"
echo "  1. Click 'Upload PDF'"
echo "  2. Select /tmp/test-book.pdf"
echo "  3. Click 'Upload'"
echo "  4. See 'Processing Complete!' message"
