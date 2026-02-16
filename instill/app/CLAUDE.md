# Code Standards for Instill

This document defines code standards for Claude Code agents implementing Instill features.

## General Principles

1. **Minimal and Focused**: Implement only what the PRD requires, nothing more
2. **Tested via Stickler**: All features must pass Stickler scenario validation
3. **Clear over Clever**: Prefer readable code to optimization
4. **TypeScript Strict**: No `any` types, use proper typing
5. **Error Handling**: Always handle edge cases and errors gracefully

## Frontend Standards (React + TypeScript)

### Component Structure

```typescript
// Use functional components with hooks
import React, { useState, useEffect } from 'react';

interface ComponentProps {
  // Always define prop types
  title: string;
  onAction: () => void;
}

export const Component: React.FC<ComponentProps> = ({ title, onAction }) => {
  const [state, setState] = useState<string>('');

  return (
    <div className="component">
      <h1>{title}</h1>
      <button onClick={onAction}>Action</button>
    </div>
  );
};
```

### File Organization

```
frontend/src/
├── components/      # Reusable UI components
├── pages/           # Route-level page components
├── api/             # API client functions
├── types/           # TypeScript type definitions
├── utils/           # Helper functions
└── App.tsx          # Root component with router
```

### Naming Conventions

- **Components**: PascalCase (e.g., `LandingPage.tsx`, `ChapterList.tsx`)
- **Files**: Match component name (e.g., `ChapterList.tsx` exports `ChapterList`)
- **Props**: Descriptive, interface named `ComponentNameProps`
- **State**: Use `useState` with descriptive names
- **CSS**: Class names are kebab-case (e.g., `chapter-list`, `button-primary`)

### API Calls

```typescript
// api/books.ts
export async function getChapters(bookId: string) {
  const response = await fetch(`/api/books/${bookId}/chapters`);
  if (!response.ok) {
    throw new Error(`Failed to fetch chapters: ${response.statusText}`);
  }
  return response.json();
}

// Usage in component
try {
  const chapters = await getChapters(bookId);
  setChapters(chapters);
} catch (error) {
  setError(error.message);
}
```

### State Management

- Use `useState` for local component state
- Use React Context for shared state (if needed across many components)
- No Redux or external state libraries for MVP

### Styling

- Use plain CSS modules or inline styles for MVP
- Keep styles in separate `.css` files
- Use semantic class names
- Mobile-first responsive design

## Backend Standards (Python + FastAPI)

### Project Structure

```
backend/
├── main.py              # FastAPI app + routes
├── models/              # Pydantic models
├── services/            # Business logic
├── storage/             # File/DB operations
└── requirements.txt
```

### Route Definitions

```python
from fastapi import FastAPI, HTTPException, UploadFile
from pydantic import BaseModel

app = FastAPI()

class ChapterResponse(BaseModel):
    id: int
    title: str
    status: str

@app.get("/api/books/{book_id}/chapters")
async def get_chapters(book_id: str) -> list[ChapterResponse]:
    """Get all chapters for a book."""
    try:
        chapters = storage.get_chapters(book_id)
        return [ChapterResponse(**ch) for ch in chapters]
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))
```

### Error Handling

- Use FastAPI's `HTTPException` for errors
- Return appropriate status codes (400, 404, 500)
- Include helpful error messages
- Log errors for debugging

### Data Storage (MVP)

- Use file system for simplicity (no database initially)
- Store data as JSON files
- Structure: `data/books/{book_id}/chapters.json`
- Create helper functions for file operations

```python
import json
from pathlib import Path

def save_book_data(book_id: str, data: dict):
    path = Path(f"data/books/{book_id}/data.json")
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, 'w') as f:
        json.dump(data, f, indent=2)

def load_book_data(book_id: str) -> dict:
    path = Path(f"data/books/{book_id}/data.json")
    if not path.exists():
        raise FileNotFoundError(f"Book {book_id} not found")
    with open(path, 'r') as f:
        return json.load(f)
```

### LLM Integration

```python
from anthropic import Anthropic
import os

client = Anthropic(api_key=os.environ.get("ANTHROPIC_API_KEY"))

async def generate_assessment(chapter_text: str) -> list[dict]:
    """Generate assessment questions using Claude."""
    message = client.messages.create(
        model="claude-3-5-sonnet-20241022",
        max_tokens=2000,
        system="You are a learning assessment specialist...",
        messages=[{
            "role": "user",
            "content": f"Generate 7 questions for:\n\n{chapter_text}"
        }]
    )
    # Parse and return questions
    return parse_questions(message.content)
```

## Testing with Stickler

### Scenario Compatibility

- Ensure all user-visible text matches scenario signals
- Use exact strings from success_signals in UI
- Avoid dynamic text that Stickler can't verify
- Add `data-testid` attributes for complex interactions

### Debugging Failed Tests

1. Check Stickler screenshots in `runs/` directory
2. Read `planning_request.json` to see what Stickler was trying
3. Verify success signals are actually visible in UI
4. Check browser console for errors

## Git Workflow

- Commit after each feature is implemented
- Clear commit messages: "Implement landing page with upload CTA"
- Don't commit `node_modules/`, `venv/`, or `data/` directories

## Performance

- No premature optimization
- Lazy load large files (split PDFs by chapter)
- Debounce user input where appropriate
- Use loading states for async operations

## Accessibility

- Use semantic HTML (`<button>`, `<nav>`, `<main>`)
- Include `alt` text for images
- Ensure keyboard navigation works
- Minimum font size 16px for readability

## Security (MVP Scope)

- No authentication in early features (single-user)
- Validate file uploads (type, size)
- Sanitize user input before LLM calls
- Don't commit API keys (use environment variables)

## Documentation

- Code comments for complex logic only
- No need for extensive docs in MVP
- PRDs serve as feature documentation
