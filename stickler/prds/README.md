# PRD Format Specification

Product Requirements Documents (PRDs) for the autonomous dev loop.

## Format

PRDs are markdown files with the following structure:

```markdown
# Feature: [Feature Name]

## Context
Brief explanation of why this feature is needed and what problem it solves.

## Requirements
- First requirement
- Second requirement
- Third requirement

## User Flow
- Step 1: User does X
- Step 2: System responds with Y
- Step 3: User sees Z

## Acceptance Criteria
- Given [context], when [action], then [expected result]
- Feature works correctly in [scenarios]
- Error handling for [edge cases]

## Technical Notes
- Implementation detail 1
- API endpoint to use
- Component to modify

## Test Data
- email: test@example.com
- password: TestPass123
- product_name: Example Product

## Success Signals
- Text that appears on success
- URL pattern for success page
- Visual element that confirms success

## Failure Signals
- Error message patterns
- Failed state indicators
- Timeout conditions
```

## Required Sections

- **Requirements**: At least one requirement
- **Acceptance Criteria**: At least one criterion
- **Success Signals**: At least one signal for verification

## Optional but Recommended

- **Context**: Helps understand the feature
- **User Flow**: Better scenario generation
- **Technical Notes**: Implementation guidance
- **Test Data**: Pre-fill test inputs
- **Failure Signals**: Error detection

## Example

See `example-logout.md` for a complete example.

## Usage

Place PRD files in this directory, then run:

```bash
npm run cli dev-loop <prd-name>
```

The system will:
1. Parse the PRD
2. Generate a Stickler scenario
3. Implement the code
4. Test with Stickler
5. Fix failures iteratively until success
