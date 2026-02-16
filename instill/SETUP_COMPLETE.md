# Instill + Stickler Setup Complete ✅

The autonomous development loop is now configured and ready to use!

## What's Been Set Up

### 1. Stickler Configuration ✅
- Modified `stickler/src/loop/agents.ts` for agent coordination
- Agents now write prompts to `agent_requests/` directory
- File-based communication with Claude Code established

### 2. Instill Workspace ✅

```
app/
├── frontend/               # React + TypeScript + Vite
│   ├── package.json
│   ├── vite.config.ts     # Dev server on :5173
│   ├── tsconfig.json
│   └── src/
│       ├── main.tsx
│       ├── App.tsx
│       └── index.css
├── backend/               # Python FastAPI
│   ├── main.py            # Server on :8000
│   ├── requirements.txt
│   └── README.md
├── CLAUDE.md              # Code standards for agents
└── README.md
```

### 3. User Story PRDs ✅

Created 16 PRD files in `stickler/prds/`:

1. **landing-page.md** - Landing page with upload CTA
2. **pdf-upload-ui.md** - File upload interface
3. **pdf-processing.md** - Backend PDF text extraction
4. **chapter-list.md** - Display chapters with lock/unlock
5. **reading-view.md** - Read chapter content
6. **assessment-trigger.md** - Start assessment flow
7. **assessment-basic.md** - Display questions one at a time
8. **assessment-grading.md** - LLM-based answer grading
9. **mastery-threshold.md** - 90% pass/fail evaluation
10. **chapter-unlock-on-pass.md** - Auto-unlock next chapter
11. **retry-on-fail.md** - Retry with new questions
12. **concept-tracking.md** - Track mastery by concept
13. **spaced-recall.md** - Inject recall questions
14. **progress-dashboard.md** - View overall progress
15. **completion-summary.md** - Final understanding report
16. **assessment-generation.md** - LLM question generation

## How to Use the Autonomous Loop

### First-Time Setup

1. **Install Frontend Dependencies**
```bash
cd /home/nickthorpe71/code/instill/app/frontend
npm install
```

2. **Install Backend Dependencies**
```bash
cd /home/nickthorpe71/code/instill/app/backend
python -m venv venv
source venv/bin/activate  # Windows: venv\\Scripts\\activate
pip install -r requirements.txt
```

3. **Set Anthropic API Key** (for Stickler's UI testing)
```bash
export ANTHROPIC_API_KEY="your-key-here"
```

### Running a User Story

For each user story, follow this pattern:

#### Example: Story 1 (landing-page)

**Step 1: Start Dev Loop**
```bash
cd /home/nickthorpe71/code/instill/stickler
npm run cli dev-loop landing-page
```

**Step 2: Execute PRD Agent Prompt**

Stickler will pause and print:
```
⏸️  PRD AGENT REQUIRED
Prompt written to: agent_requests/prd_agent_request.txt
```

Read that file, paste the prompt into this Claude Code chat, and I'll:
- Create the Stickler scenario at `scenarios/landing-page.json`
- Report back when done

**Step 3: Execute Code Agent Prompt**

Stickler continues and prints:
```
⏸️  CODE AGENT REQUIRED
Prompt written to: agent_requests/code_agent_request.txt
```

Paste that prompt to me, and I'll:
- Implement the landing page feature
- Create/modify files in `app/frontend/src/`
- Report when implementation is complete

**Step 4: Start Frontend** (if not already running)
```bash
cd /home/nickthorpe71/code/instill/app/frontend
npm run dev
```

**Step 5: Re-run Dev Loop for Testing**

Stickler will now run the automated UI test:
- Launches Playwright browser
- Navigates to http://localhost:5173
- Pauses for planning decisions
- Writes `planning_request.json` files

**Step 6: Execute Planning Prompts**

When Stickler pauses for UI decisions, it writes:
```
stickler/runs/{timestamp}_landing-page/planning_request.json
```

Read and paste to me, and I'll:
- Analyze the screenshot
- Decide next action (click, type, verify)
- Write response to `planning_response.json`

**Step 7: Test Continues**

Stickler reads my response and continues testing until:
- ✅ **PASS**: Feature works correctly
- ❌ **FAIL**: Writes fix agent prompt

If failed, paste the fix agent prompt to me and I'll debug and fix.

### Iterative Flow

```
1. Run dev-loop <story> → 2. Paste PRD prompt to Claude →
3. Paste Code prompt to Claude → 4. Start frontend →
5. Re-run dev-loop → 6. Paste planning prompts to Claude →
7. Test passes ✅ → Move to next story
```

## Current Status

- **Story 1 (landing-page)**: Ready to run
- **Stories 2-16**: Ready in sequence
- **Workspace**: Initialized
- **Stickler**: Configured

## Next Steps

1. Start with Story 1: `npm run cli dev-loop landing-page`
2. Paste agent prompts to this chat
3. I'll implement and guide through testing
4. Once Story 1 passes, move to Story 2
5. Repeat for all 16 stories → MVP complete!

## Tips

- **Check screenshots**: When tests fail, screenshots are in `stickler/runs/`
- **Read planning requests**: They show exactly what Stickler sees
- **Incremental**: Each story builds on the previous
- **Manual checkpoints**: You control when to move to next story
- **Pause anytime**: Ctrl+C to stop, re-run to continue

## Troubleshooting

- **Port 5173 in use**: Kill other processes or change port in vite.config.ts
- **API key missing**: Stickler needs ANTHROPIC_API_KEY for UI testing
- **Planning timeout**: If I don't respond in 5 minutes, Stickler times out
- **Test failures**: Check browser is headless-compatible (runs fine in WSL)

Ready to build Instill! 🚀
