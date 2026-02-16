# Instill - Learning Mastery System

Transform passive reading into verified understanding through adaptive assessments and enforced progression.

## Project Structure

```
app/
├── frontend/       # React + TypeScript + Vite
│   └── src/
├── backend/        # Python FastAPI
└── README.md
```

## Quick Start

### Frontend (http://localhost:5173)

```bash
cd frontend
npm install
npm run dev
```

### Backend (http://localhost:8000)

```bash
cd backend
python -m venv venv
source venv/bin/activate  # Windows: venv\Scripts\activate
pip install -r requirements.txt
python main.py
```

## Development

The app is being built incrementally using **Stickler** - an autonomous UI testing framework.

Each feature is developed as a user story with:
1. PRD defining requirements
2. Automated scenario generation
3. Feature implementation
4. Stickler UI validation

See `../stickler/prds/` for feature PRDs.

## Tech Stack

- **Frontend**: React 18, TypeScript, Vite, React Router
- **Backend**: Python, FastAPI, Uvicorn
- **LLM**: Anthropic Claude (for assessment generation)
- **Testing**: Stickler (autonomous UI validation)
