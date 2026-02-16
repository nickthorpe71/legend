#!/bin/bash

# Stickler New Project Initializer
# Usage: ./init-new-project.sh <project-name> [port]

set -e

if [ -z "$1" ]; then
  echo "Usage: ./init-new-project.sh <project-name> [port]"
  echo "Example: ./init-new-project.sh my-app 3000"
  exit 1
fi

PROJECT_NAME=$1
PORT=${2:-3000}
BASE_URL="http://localhost:$PORT"

echo "🚀 Initializing Stickler for new project: $PROJECT_NAME"
echo "   Base URL: $BASE_URL"
echo ""

# Clean old project-specific data
echo "🧹 Cleaning old project data..."
rm -rf scenarios/*.json
rm -rf runs/*
rm -rf prds/*.md

# Update config
echo "⚙️  Updating configuration..."
cat > stickler.config.json << EOF
{
  "baseURL": "$BASE_URL",
  "viewport": {
    "width": 1280,
    "height": 720
  },
  "timeouts": {
    "pageLoad": 30000,
    "action": 5000,
    "maxRunDuration": 300000
  },
  "maxSteps": 50,
  "actionDelay": {
    "min": 50,
    "max": 100
  },
  "llm": {
    "provider": "anthropic",
    "model": "claude-3-5-sonnet-20241022",
    "maxTokens": 2000,
    "temperature": 0.0
  }
}
EOF

# Install dependencies if needed
if [ ! -d "node_modules" ]; then
  echo "📦 Installing dependencies..."
  npm install
fi

# Install Playwright browsers if needed
if [ ! -d "$HOME/.cache/ms-playwright" ]; then
  echo "🎭 Installing Playwright browsers..."
  npx playwright install chromium
fi

# Create example PRD
echo "📝 Creating example PRD..."
cat > prds/example-homepage.md << 'EOF'
# Feature: Homepage

## Context
Create a simple homepage for the application.

## Requirements
- Display app title
- Show welcome message
- Include a call-to-action button

## User Flow
- User navigates to homepage
- User sees title and welcome message
- User sees CTA button

## Acceptance Criteria
- Title is visible and centered
- Welcome message displays below title
- CTA button is clickable

## Technical Notes
- Use semantic HTML
- Center content vertically and horizontally
- Add basic styling

## Test Data
- N/A (static page)

## Success Signals
- Welcome
- Get Started
- Homepage

## Failure Signals
- Error
- Not found
- 404
EOF

echo ""
echo "✅ Stickler initialized for $PROJECT_NAME!"
echo ""
echo "Next steps:"
echo "  1. Start your app on $BASE_URL"
echo "  2. Edit prds/example-homepage.md (or create new PRD)"
echo "  3. Run: npm run cli dev-loop example-homepage"
echo ""
echo "Commands:"
echo "  npm run cli dev-loop <prd-name>  # Build feature from PRD"
echo "  npm run cli list                 # List scenarios"
echo "  npm run cli run <scenario>       # Test specific scenario"
echo ""
