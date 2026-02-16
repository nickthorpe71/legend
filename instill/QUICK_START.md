# Quick Start Guide

## One-Time Setup

```bash
# 1. Install frontend
cd /home/nickthorpe71/code/instill/app/frontend
npm install

# 2. Install backend
cd /home/nickthorpe71/code/instill/app/backend
python -m venv venv
source venv/bin/activate
pip install -r requirements.txt

# 3. Set API key
export ANTHROPIC_API_KEY="your-key-here"
```

## Run Story 1

```bash
# Start Stickler dev loop
cd /home/nickthorpe71/code/instill/stickler
npm run cli dev-loop landing-page
```

When it pauses:
1. Read `agent_requests/prd_agent_request.txt`
2. Paste to Claude Code chat
3. Wait for completion
4. Repeat for code agent prompt
5. Start frontend: `cd ../app/frontend && npm run dev`
6. Re-run dev-loop
7. Paste planning prompts as they appear

## Story Order

1. landing-page
2. pdf-upload-ui
3. pdf-processing
4. chapter-list
5. reading-view
6. assessment-trigger
7. assessment-basic
8. assessment-grading
9. mastery-threshold
10. chapter-unlock-on-pass
11. retry-on-fail
12. concept-tracking
13. spaced-recall
14. progress-dashboard
15. completion-summary
16. assessment-generation

## Commands

```bash
# Run dev loop
npm run cli dev-loop <story-name>

# List scenarios
npm run cli list

# Start frontend
cd app/frontend && npm run dev

# Start backend
cd app/backend && source venv/bin/activate && python main.py
```

That's it! You're ready to build. 🚀
