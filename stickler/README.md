# Stickler - Autonomous UI Validator

Stickler is an autonomous UI testing tool designed for **agent-driven development**. It creates a feedback loop between Claude Code and your application's UI, enabling self-healing development workflows.

## How It Works

1. **You give Claude Code a task** (e.g., "Fix the login flow")
2. **Claude Code implements the changes**
3. **Stickler autonomously tests the UI** by:
   - Observing UI state (screenshot + DOM)
   - Planning next action (asks Claude Code what to do)
   - Executing actions (human-like mouse/keyboard)
   - Verifying success/failure conditions
4. **On failure**: Stickler generates a structured issue with screenshots and trace
5. **Claude Code reads the issue** and fixes the problem
6. **Repeat until the scenario passes** ✅

## Quick Start

### 1. Install Dependencies

```bash
cd stickler
npm install
npx playwright install chromium
```

### 2. Create a Scenario

```bash
npm run cli init my-scenario
```

Edit `scenarios/my-scenario.json`:

```json
{
  "name": "my-scenario",
  "start_url": "http://localhost:5173/some-page",
  "objective": "What you want to achieve",
  "success_signals": ["text that indicates success", "another signal"],
  "fail_signals": ["error", "failed"],
  "max_steps": 20
}
```

### 3. Run the Scenario

```bash
npm run cli run my-scenario
```

### 4. Interact with Claude Code

When Stickler pauses for planning:

1. Look at the planning request file (shown in console)
2. View the screenshot
3. Ask Claude Code: _"Review the planning request and provide the next action"_
4. Claude Code writes `planning_response.json`
5. Stickler continues execution

### 5. Review Results

After the run:

- ✅ **PASS**: Check `runs/{timestamp}_{scenario}/verdict.json`
- ❌ **FAIL**: Check `runs/{timestamp}_{scenario}/issue.json` and `fix.md`

## CLI Commands

```bash
# List available scenarios
npm run cli list

# Run a scenario
npm run cli run <scenario-name>

# Create a new scenario template
npm run cli init <scenario-name>
```

## Project Structure

```
stickler/
├── scenarios/           # Scenario definitions
│   └── login.json      # Example login scenario
├── runs/               # Test run artifacts (auto-generated)
│   └── {timestamp}_{scenario}/
│       ├── verdict.json          # Final result
│       ├── issue.json            # Failure details (if failed)
│       ├── fix.md                # Claude Code prompt (if failed)
│       ├── trace.jsonl           # Action history
│       ├── planning_request.json # Current planning request
│       └── screenshots/          # Step-by-step screenshots
├── src/
│   ├── cli.ts          # CLI entry point
│   ├── runner.ts       # Main test loop
│   ├── scenario.ts     # Scenario loading
│   ├── artifacts.ts    # Result generation
│   └── ui/
│       ├── observer.ts # UI state capture
│       ├── planner.ts  # Decision making (with Claude Code)
│       ├── actor.ts    # Action execution
│       ├── verifier.ts # Success/fail checking
│       └── types.ts    # Type definitions
└── stickler.config.json # Global configuration
```

## Example Workflow

### Scenario: Fix Login Flow

1. **User**: "Claude, fix the login flow so users can sign in"

2. **Claude Code**: Makes changes to login components

3. **User**: "Run Stickler to test it"

4. **Run Stickler**:
   ```bash
   npm run cli run login
   ```

5. **Stickler pauses at each step**:
   - Shows screenshot
   - Shows planning request
   - Waits for Claude Code's decision

6. **User asks Claude Code**:
   _"Review the planning request and screenshot, then provide the next action"_

7. **Claude Code**:
   - Reads `planning_request.json`
   - Views screenshot
   - Writes decision to `planning_response.json`:
   ```json
   {
     "type": "type",
     "target": "Email",
     "text": "test@example.com",
     "reason": "Filling the email field to begin login"
   }
   ```

8. **Stickler executes** and continues to next step

9. **On failure**: Stickler creates `issue.json` and `fix.md`

10. **Claude Code**: Reads the issue, fixes the code, and we run again

## Configuration

Edit `stickler.config.json`:

```json
{
  "baseURL": "http://localhost:5173",
  "viewport": { "width": 1280, "height": 720 },
  "timeouts": {
    "pageLoad": 30000,
    "action": 5000,
    "maxRunDuration": 300000
  },
  "maxSteps": 50,
  "actionDelay": { "min": 100, "max": 500 }
}
```

## Scenario Definition

```json
{
  "name": "scenario-name",
  "start_url": "http://localhost:5173/page",
  "objective": "Clear description of what to achieve",
  "success_signals": [
    "Text that appears on success",
    "Button label that indicates completion"
  ],
  "success_url_contains": [
    "/success",
    "/dashboard"
  ],
  "fail_signals": [
    "Error message",
    "Failed state indicator"
  ],
  "fail_url_contains": [
    "/error",
    "/login"
  ],
  "max_steps": 20,
  "context": "Optional additional context for Claude Code",
  "test_data": {
    "email": "test@example.com",
    "password": "password123"
  }
}
```

### Field Descriptions

- **name**: Unique identifier for the scenario
- **start_url**: Where to begin the test
- **objective**: What you're trying to achieve (for Claude Code's understanding)
- **success_signals**: Text/labels that must be visible on success
- **success_url_contains**: URL patterns that indicate success (e.g., `/app/`, `/dashboard`)
- **fail_signals**: Text/labels that indicate failure
- **fail_url_contains**: URL patterns that indicate failure (e.g., `/error`, `/login` after attempting auth)
- **max_steps**: Maximum actions before timeout
- **context**: Additional information for Claude Code
- **test_data**: Credentials or form values to use during testing

## Tips for Writing Good Scenarios

1. **Clear objective**: Be specific about what success looks like
2. **Use URL checks for navigation**: `success_url_contains` and `fail_url_contains` are perfect for verifying redirects
3. **Unique signals**: Choose text that only appears on success/failure
4. **Combine signals**: Use both text and URL checks for robust verification
5. **Reasonable max_steps**: Estimate how many interactions are needed
6. **Add context**: Help Claude Code understand the scenario's purpose
7. **Include test_data**: Provide credentials or form values so Claude Code knows what to type

## Debugging

- **Screenshots**: Check `runs/{timestamp}_{scenario}/screenshots/`
- **Trace**: Review `trace.jsonl` for full action history
- **Planning requests**: See what Claude Code was asked to do
- **Issue files**: Read `issue.json` and `fix.md` for failure analysis

## Integration with Git

Add to `.gitignore`:
```
stickler/runs/
stickler/node_modules/
```

Commit scenarios:
```
stickler/scenarios/*.json
```

## Future Enhancements

- [ ] Visual regression testing (baseline screenshots)
- [ ] Parallel scenario execution
- [ ] Custom assertion DSL
- [ ] CI/CD integration
- [ ] Web UI for viewing run history

## Architecture

Stickler uses a simple **Observe → Plan → Act → Verify** loop:

1. **Observer**: Captures UI state (screenshot + interactable elements + visible text)
2. **Planner**: Asks Claude Code to decide the next action based on scenario + UI state
3. **Actor**: Executes actions with human-like mouse/keyboard behavior
4. **Verifier**: Checks if success/fail signals are present

This creates an autonomous agent that can navigate your UI while maintaining human-like interactions.
