# Dev Loop Guide for Claude Code

This guide explains how to use Stickler's autonomous development loop.

## Quick Start

```bash
cd /home/nickthorpe71/projects/scrapingbee_mvp/stickler
npm run cli dev-loop <prd-name>
```

## What is the Dev Loop?

The dev loop is an **autonomous development cycle** where Claude Code:

1. **Reads a PRD** (Product Requirements Document)
2. **Generates a Stickler scenario** to test the feature
3. **Implements the code** to satisfy the PRD
4. **Runs Stickler tests** to verify implementation
5. **Fixes failures iteratively** until tests pass

This creates a **reflection loop** where code is written, tested, and refined automatically.

## Current Mode: Manual (v1.0)

The current implementation runs in **manual mode**:
- The orchestrator prints detailed prompts
- Claude Code (you) should execute each prompt as described
- After completing each phase, you can continue or stop

### Manual Mode Workflow

1. **Start dev loop**:
   ```bash
   npm run cli dev-loop example-logout
   ```

2. **PRD Agent Prompt** - Orchestrator prints a detailed prompt for generating the scenario:
   - Read the prompt carefully
   - Create the scenario JSON file as described
   - Save to the specified path

3. **Code Agent Prompt** - Orchestrator prints implementation instructions:
   - Analyze the codebase
   - Implement backend and frontend code
   - Follow the PRD requirements exactly

4. **Test Execution** - Orchestrator runs Stickler automatically:
   - If PASS: Success! Done.
   - If FAIL: Fix Agent prompt is printed

5. **Fix Agent Prompt** (on failure) - Orchestrator prints debugging instructions:
   - Review test artifacts (screenshots, logs)
   - Identify root cause
   - Apply minimal fix
   - Loop continues automatically

## PRD Format

PRDs are markdown files in `prds/` directory:

```markdown
# Feature: [Name]

## Context
Why this feature is needed

## Requirements
- Requirement 1
- Requirement 2

## User Flow
- Step 1: User does X
- Step 2: System does Y

## Acceptance Criteria
- Given [context], when [action], then [result]

## Technical Notes
- Implementation details
- API endpoints to use

## Test Data
- email: test@example.com
- password: TestPass123

## Success Signals
- Text that appears on success
- URL patterns

## Failure Signals
- Error messages
- Failed states
```

See `prds/example-logout.md` for a complete example.

## Creating a PRD

1. Create a new `.md` file in `prds/` directory
2. Follow the format above
3. Be specific about:
   - User flow (exact steps)
   - Success signals (text/URLs to verify)
   - Test data (credentials, inputs)

## Running the Dev Loop

```bash
npm run cli dev-loop <prd-name>

# Options:
--max-iterations <number>     # Max iterations (default: 5)
--max-same-error <number>     # Stop if same error repeats (default: 3)
--verbose                     # Verbose logging
--workspace <path>            # Workspace root (default: ../)
```

### Example:

```bash
npm run cli dev-loop example-logout --max-iterations 7 --verbose
```

## Understanding the Output

### Loop Report

```
═══════════════════════════════════════════════════════
                    DEV LOOP REPORT
═══════════════════════════════════════════════════════

PRD: example-logout
Status: ✅ SUCCESS
Iterations: 3
Duration: 45.2s

Scenario: scenarios/example-logout.json

Iteration History:
  ✅ [1] Generate Scenario (2.1s)
  ✅ [2] Implement Code (38.5s)
  ✅ [3] Run Test (4.6s)

✅ Feature implemented and tested successfully!
```

### Iteration Phases

- **Generate Scenario**: PRD → Stickler scenario JSON
- **Implement Code**: Write backend + frontend code
- **Run Test**: Execute Stickler scenario
- **Apply Fix**: Debug and fix failures

### Status Codes

- ✅ **SUCCESS**: Feature complete, all tests pass
- ❌ **FAILED**: Max iterations reached without success
- 🔴 **STUCK**: Same error repeated 3+ times

## Stuck Detection

The loop detects when you're stuck:
- Same error repeats 3+ times
- Likely indicates a fundamental issue
- Loop stops to avoid wasting time

Error signatures are normalized:
- Numbers replaced with `N`
- File paths generalized
- Line numbers ignored

## Example: Complete Dev Loop Flow

```bash
# 1. Create PRD
cat > prds/add-profile-page.md << 'EOF'
# Feature: User Profile Page

## Context
Users need to view and edit their profile information.

## Requirements
- Display user email and name
- Allow editing name
- Save changes to backend

## User Flow
- Navigate to /profile
- See current profile info
- Click edit, change name
- Click save
- See success message

## Acceptance Criteria
- Profile loads with correct user data
- Name can be edited and saved
- Changes persist after page refresh

## Test Data
- email: test@example.com
- password: Pass123
- new_name: John Doe

## Success Signals
- Profile saved
- Success
- Updated

## Failure Signals
- Error
- Failed
EOF

# 2. Run dev loop
npm run cli dev-loop add-profile-page

# 3. Follow prompts:
#    - Create scenario from PRD agent prompt
#    - Implement code from code agent prompt
#    - Review test results
#    - Apply fixes if needed
```

## Tips for Success

### Writing Good PRDs

✅ **DO**:
- Be specific about user flow
- Include exact test data
- Define clear success signals
- Specify URLs and text to verify

❌ **DON'T**:
- Be vague about steps
- Forget test credentials
- Use ambiguous success criteria

### During Implementation

✅ **DO**:
- Follow the prompts exactly
- Read test artifacts carefully
- Apply minimal, targeted fixes
- Trust the stuck detection

❌ **DON'T**:
- Skip steps or rush
- Ignore test screenshots
- Over-engineer solutions
- Keep trying the same fix

## Future: Automatic Mode (v2.0)

The next version will use the **Task tool** to launch agents automatically:

```typescript
// PRD Agent
const scenarioPath = await Task({
  subagent_type: 'general-purpose',
  prompt: buildPRDAgentPrompt(prd)
});

// Code Agent
await Task({
  subagent_type: 'general-purpose',
  prompt: buildCodeAgentPrompt(prd, scenarioPath)
});

// Fix Agent
await Task({
  subagent_type: 'general-purpose',
  prompt: buildFixAgentPrompt(runDir, previousAttempts)
});
```

This will enable **fully autonomous development** with no manual intervention.

## Architecture

```
USER provides PRD
  ↓
ORCHESTRATOR validates PRD
  ↓
PRD AGENT generates scenario
  ↓
CODE AGENT implements feature
  ↓
STICKLER tests implementation
  ↓
  ├─ PASS → Success!
  └─ FAIL → FIX AGENT analyzes & fixes
                ↓
                Loop back to STICKLER
                (max 5 iterations or stuck detection)
```

## Files Structure

```
stickler/
├── prds/                          # PRD documents
│   ├── README.md                  # PRD format spec
│   └── example-logout.md          # Example PRD
├── scenarios/                     # Generated scenarios
│   └── example-logout.json        # From PRD
├── src/
│   ├── prd/                       # PRD processing
│   │   ├── types.ts
│   │   ├── processor.ts           # Parse markdown
│   │   └── validator.ts           # Validate completeness
│   └── loop/                      # Dev loop orchestration
│       ├── types.ts
│       ├── orchestrator.ts        # Main loop
│       ├── tracker.ts             # Iteration tracking
│       ├── reporter.ts            # Progress reporting
│       └── agents.ts              # Agent prompts
└── .ai-docs/
    └── dev-loop-guide.md          # This file
```

## Troubleshooting

### "PRD not found"
- Ensure PRD file is in `prds/` directory
- Check filename matches command (without .md extension)

### "Scenario not found" during test
- PRD agent didn't create the scenario file
- Check the expected path in error message
- Manually create scenario if needed

### Loop stuck on same error
- Review test screenshots in `runs/` directory
- Check if implementation matches PRD exactly
- Consider if PRD requirements are achievable
- May need to revise PRD

### Test passes but feature incomplete
- Success signals may be too broad
- Refine success_signals in PRD
- Add more specific acceptance criteria

## Related Docs

- [Running Tests](.ai-docs/running-tests.md) - How to run Stickler tests manually
- [PRD Format](../prds/README.md) - Complete PRD specification
- [Dev Loop Plan](../DEV_LOOP_IMPLEMENTATION.md) - Original implementation plan
