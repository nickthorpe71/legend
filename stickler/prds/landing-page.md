# Feature: Landing Page

## Context
Users need a clear entry point that explains Instill's value proposition and provides a path to upload their first PDF. This is the first impression of the product.

## Requirements
- Display "Instill" branding/logo
- Show tagline explaining mastery-based learning
- Prominent "Upload PDF" call-to-action button
- Clean, focused design with no distractions
- Responsive layout (desktop and mobile)

## User Flow
- Step 1: User navigates to http://localhost:5173/
- Step 2: User sees landing page with Instill branding
- Step 3: User reads value proposition
- Step 4: User sees "Upload PDF" button (ready to click)

## Acceptance Criteria
- Given a user visits the root URL, then they see the Instill landing page
- The page displays "Instill" as the main heading
- A tagline is visible explaining the learning mastery concept
- An "Upload PDF" or "Get Started" button is prominently displayed
- The page is responsive and works on mobile browsers

## Technical Notes
- Create landing page component in React
- Use semantic HTML (h1, p, button elements)
- Add basic CSS for centering and typography
- No backend integration needed yet (button can be non-functional)
- Consider using flexbox/grid for responsive layout

## Test Data
- None required (static page)

## Success Signals
- Instill
- Upload PDF
- Get Started
- master
- learning

## Failure Signals
- Error
- 404
- Not Found
