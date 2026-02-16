# Stickler Quick Start - Build Any Project

Use Stickler to autonomously build a new web project from a PRD.

## Setup (5 minutes)

```bash
# 1. Copy Stickler to your new project
cp -r stickler ~/my-new-project/

# 2. Install dependencies
cd ~/my-new-project/stickler
npm install
npx playwright install chromium

# 3. Update config for your app
nano stickler.config.json
# Change baseURL to your dev server (e.g., "http://localhost:3000")

# Done! Stickler is ready.
```

## Build a Project from PRD

### 1. Write a PRD

Create `prds/my-feature.md`:

```markdown
# Feature: Todo List App

## Context
Build a simple todo list application with add/delete functionality.

## Requirements
- Display list of todos
- Add new todos via input field
- Delete todos by clicking X button
- Todos persist in localStorage

## User Flow
- User sees empty todo list
- User types "Buy milk" in input field
- User clicks "Add" button
- Todo appears in list
- User clicks X next to todo
- Todo disappears

## Acceptance Criteria
- Todos display in a list
- Add button creates new todo
- Delete button removes todo
- Page refresh preserves todos

## Technical Notes
- Use React + Vite
- Style with Tailwind CSS
- Store in localStorage

## Test Data
- todo1: Buy milk
- todo2: Walk dog
- todo3: Write code

## Success Signals
- Buy milk
- Walk dog
- todos

## Failure Signals
- Error
- Failed
- undefined
```

### 2. Run Dev Loop

```bash
npm run cli dev-loop my-feature
```

### 3. Follow the Prompts

Stickler will guide you through:

**Phase 1: Generate Scenario** - Creates test scenario from PRD
**Phase 2: Implement Code** - Builds the feature
**Phase 3: Test** - Runs Stickler to verify
**Phase 4: Fix** - Iterates until tests pass

### 4. Watch It Build

The loop will:
1. Generate a Stickler scenario from your PRD
2. Implement the code (frontend + backend)
3. Test with Stickler
4. Fix failures automatically
5. Repeat until success ✅

## Example: Build a Login Page

```bash
# 1. Create PRD
cat > prds/login-page.md << 'EOF'
# Feature: User Login

## Context
Users need to log into the application.

## Requirements
- Email and password input fields
- Submit button
- Show error on invalid credentials
- Redirect to /dashboard on success

## User Flow
- User navigates to /login
- User enters email and password
- User clicks "Sign In"
- System validates credentials
- Success: redirect to /dashboard
- Failure: show error message

## Acceptance Criteria
- Login form displays on /login
- Valid credentials redirect to /dashboard
- Invalid credentials show error
- Password field is masked

## Technical Notes
- Create /login route
- Add authentication API endpoint
- Use JWT for session
- Hash passwords

## Test Data
- valid_email: test@example.com
- valid_password: password123
- invalid_email: wrong@example.com
- invalid_password: wrongpass

## Success Signals
- Dashboard
- Welcome
- Logged in

## Failure Signals
- Invalid credentials
- Login failed
- Error
EOF

# 2. Run dev loop
npm run cli dev-loop login-page

# 3. Stickler builds the entire feature!
```

## What Stickler Builds For You

Given a PRD, Stickler will:

✅ **Frontend**:
- React components
- Form handling
- Routing
- State management
- Styling

✅ **Backend**:
- API endpoints
- Database models
- Authentication
- Validation
- Error handling

✅ **Testing**:
- Generates test scenarios
- Runs UI tests
- Verifies functionality
- Fixes bugs iteratively

## Tips for Success

### Write Clear PRDs

✅ **Good PRD**:
```markdown
## User Flow
- User clicks "Add Todo"
- Input field appears
- User types "Buy milk"
- User presses Enter
- Todo appears in list with text "Buy milk"
```

❌ **Vague PRD**:
```markdown
## User Flow
- User adds a todo
- It shows up
```

### Include Exact Success Signals

✅ **Good Signals**:
```
Success Signals: Dashboard, Welcome back, Logged in successfully
```

❌ **Vague Signals**:
```
Success Signals: Success
```

### Provide Test Data

✅ **Good Test Data**:
```
- email: test@example.com
- password: TestPass123
- todo_text: Buy groceries
```

❌ **No Test Data**:
```
(empty)
```

## Starting Fresh Project

```bash
# 1. Create project directory
mkdir my-awesome-app
cd my-awesome-app

# 2. Copy Stickler
cp -r /path/to/stickler .

# 3. Clean old data
cd stickler
rm -rf scenarios/*.json runs/* prds/*.md

# 4. Update config
echo '{
  "baseURL": "http://localhost:3000",
  "viewport": { "width": 1280, "height": 720 },
  "timeouts": { "pageLoad": 30000, "action": 5000, "maxRunDuration": 300000 },
  "maxSteps": 50,
  "actionDelay": { "min": 50, "max": 100 },
  "llm": {
    "provider": "anthropic",
    "model": "claude-3-5-sonnet-20241022",
    "maxTokens": 2000,
    "temperature": 0.0
  }
}' > stickler.config.json

# 5. Install
npm install
npx playwright install chromium

# 6. Setup your app
cd ..
npm create vite@latest app -- --template react-ts
cd app
npm install
npm run dev  # Starts on localhost:3000

# 7. Write PRD and build!
cd ../stickler
nano prds/homepage.md  # Write your first feature
npm run cli dev-loop homepage
```

## Workflow

```
┌─────────────────────────────────────────┐
│  1. Write PRD (prds/feature.md)        │
└──────────────┬──────────────────────────┘
               │
               ▼
┌─────────────────────────────────────────┐
│  2. Run: npm run cli dev-loop feature  │
└──────────────┬──────────────────────────┘
               │
               ▼
┌─────────────────────────────────────────┐
│  3. Stickler generates scenario        │
└──────────────┬──────────────────────────┘
               │
               ▼
┌─────────────────────────────────────────┐
│  4. Stickler implements code           │
└──────────────┬──────────────────────────┘
               │
               ▼
┌─────────────────────────────────────────┐
│  5. Stickler tests implementation      │
└──────────────┬──────────────────────────┘
               │
         ┌─────┴─────┐
         │           │
    ✅ PASS      ❌ FAIL
         │           │
         │           ▼
         │    ┌──────────────┐
         │    │  6. Fix code │
         │    └──────┬───────┘
         │           │
         │           ▼
         │      Back to step 5
         │
         ▼
    🎉 Done!
```

## Commands Reference

```bash
# Start dev loop
npm run cli dev-loop <prd-name>

# List scenarios
npm run cli list

# Run specific test
npm run cli run <scenario-name>

# Clean old test runs
npm run cli clean --all

# Create scenario manually
npm run cli init <scenario-name>
```

## Project Structure

```
my-new-project/
├── app/                    # Your application code
│   ├── src/
│   ├── public/
│   └── package.json
│
└── stickler/              # Stickler testing
    ├── prds/              # Write PRDs here
    │   └── feature.md
    ├── scenarios/         # Auto-generated tests
    ├── runs/              # Test results
    └── stickler.config.json
```

## What's Next?

Once Stickler builds your project:

1. **Review the code** - Check what was generated
2. **Run the app** - `cd app && npm run dev`
3. **Test manually** - Verify in browser
4. **Iterate** - Write more PRDs for new features

## Need Help?

- PRD format: See `prds/README.md`
- Dev loop guide: See `.ai-docs/dev-loop-guide.md`
- Running tests: See `.ai-docs/running-tests.md`
