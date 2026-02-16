/**
 * Agent Coordination - Launch specialized Claude Code agents
 */

import * as fs from 'fs';
import * as path from 'path';
import { ParsedPRD } from '../prd/types.js';
import { IterationRecord } from './types.js';

// Note: In Node.js runtime, we cannot directly import Claude Code's Task tool
// Instead, we'll use a workaround with file-based communication
// Claude Code will need to monitor these files and respond

export async function launchPRDAgent(
  prdPath: string,
  prd: ParsedPRD,
  scenariosDir: string
): Promise<string> {
  console.log('🤖 Launching PRD Agent to generate scenario...');

  const prompt = buildPRDAgentPrompt(prd, scenariosDir);

  // Write agent request to file for Claude Code to pick up
  const agentRequestPath = path.join(process.cwd(), 'agent_requests', 'prd_agent_request.txt');
  await fs.promises.mkdir(path.dirname(agentRequestPath), { recursive: true });
  await fs.promises.writeFile(agentRequestPath, prompt);

  console.log('\n' + '='.repeat(80));
  console.log('⏸️  PRD AGENT REQUIRED');
  console.log('='.repeat(80));
  console.log('\nPrompt written to:', agentRequestPath);
  console.log('\nPlease paste this prompt to Claude Code to generate the scenario.');
  console.log('Press Ctrl+C when done, then re-run to continue.');
  console.log('='.repeat(80) + '\n');

  // For manual mode, just return the prompt
  return prompt;
}

export async function launchCodeAgent(
  prd: ParsedPRD,
  scenarioPath: string,
  workspaceRoot: string
): Promise<string> {
  console.log('🤖 Launching Code Agent to implement feature...');

  const prompt = buildCodeAgentPrompt(prd, scenarioPath, workspaceRoot);

  // Write agent request to file for Claude Code to pick up
  const agentRequestPath = path.join(process.cwd(), 'agent_requests', 'code_agent_request.txt');
  await fs.promises.mkdir(path.dirname(agentRequestPath), { recursive: true });
  await fs.promises.writeFile(agentRequestPath, prompt);

  console.log('\n' + '='.repeat(80));
  console.log('⏸️  CODE AGENT REQUIRED');
  console.log('='.repeat(80));
  console.log('\nPrompt written to:', agentRequestPath);
  console.log('\nPlease paste this prompt to Claude Code to implement the feature.');
  console.log('Press Ctrl+C when done, then re-run to continue.');
  console.log('='.repeat(80) + '\n');

  // For manual mode, just return the prompt
  return prompt;
}

export async function launchFixAgent(
  runDir: string,
  iteration: number,
  previousAttempts: IterationRecord[],
  prd: ParsedPRD
): Promise<string> {
  console.log('🤖 Launching Fix Agent to analyze failure and apply fix...');

  const prompt = buildFixAgentPrompt(runDir, iteration, previousAttempts, prd);

  // Write agent request to file for Claude Code to pick up
  const agentRequestPath = path.join(process.cwd(), 'agent_requests', 'fix_agent_request.txt');
  await fs.promises.mkdir(path.dirname(agentRequestPath), { recursive: true });
  await fs.promises.writeFile(agentRequestPath, prompt);

  console.log('\n' + '='.repeat(80));
  console.log('⏸️  FIX AGENT REQUIRED');
  console.log('='.repeat(80));
  console.log('\nPrompt written to:', agentRequestPath);
  console.log('\nPlease paste this prompt to Claude Code to fix the issue.');
  console.log('Press Ctrl+C when done, then re-run to continue.');
  console.log('='.repeat(80) + '\n');

  // For manual mode, just return the prompt
  return prompt;
}

function buildPRDAgentPrompt(prd: ParsedPRD, scenariosDir: string): string {
  return `
# Task: Generate Stickler Scenario from PRD

You are a specialized agent that converts Product Requirements Documents into Stickler test scenarios.

## PRD Summary

**Feature**: ${prd.name}

**Context**: ${prd.context}

**Requirements**:
${prd.requirements.map(r => `- ${r}`).join('\n')}

**User Flow**:
${prd.userFlow.map(f => `- ${f}`).join('\n')}

**Acceptance Criteria**:
${prd.acceptanceCriteria.map(a => `- ${a}`).join('\n')}

**Test Data**:
${Object.entries(prd.testData).map(([k, v]) => `- ${k}: ${v}`).join('\n')}

**Success Signals**: ${prd.successSignals.join(', ')}
**Failure Signals**: ${prd.failureSignals.join(', ')}

## Your Task

Generate a Stickler scenario JSON file that tests this feature. The scenario should:

1. Start at the appropriate URL (likely http://localhost:5173/ or http://localhost:5173/login)
2. Include all necessary steps from the user flow
3. Use the test data provided
4. Define clear success signals and URL patterns
5. Be saved to: ${scenariosDir}/${prd.name}.json

## Stickler Scenario Format

{
  "name": "${prd.name}",
  "start_url": "http://localhost:5173/",
  "objective": "Clear description of what this scenario tests",
  "success_signals": ["signal1", "signal2"],
  "success_url_contains": ["/expected/path"],
  "failure_signals": ["error", "failed"],
  "test_data": {
    "field1": "value1"
  }
}

## Example Reference

Look at existing scenarios in ${scenariosDir}/ for examples:
- login.json
- live-query.json
- example-logout.json

## Output

Create the scenario file and report back:
- Path to created file
- Brief description of test flow
- Any assumptions made
`.trim();
}

function buildCodeAgentPrompt(prd: ParsedPRD, scenarioPath: string, workspaceRoot: string): string {
  return `
# Task: Implement Feature from PRD

You are a specialized agent that implements features based on Product Requirements Documents.

## PRD Summary

**Feature**: ${prd.name}

**Requirements**:
${prd.requirements.map(r => `- ${r}`).join('\n')}

**User Flow**:
${prd.userFlow.map(f => `- ${f}`).join('\n')}

**Acceptance Criteria**:
${prd.acceptanceCriteria.map(a => `- ${a}`).join('\n')}

**Technical Notes**:
${prd.technicalNotes.map(n => `- ${n}`).join('\n')}

## Test Scenario

A Stickler scenario has been created at: ${scenarioPath}

This scenario will be used to verify your implementation.

## Your Task

Implement this feature in the codebase at: ${workspaceRoot}

### Implementation Steps:

1. **Analyze the codebase** - Understand current structure
2. **Implement backend** - API routes, handlers, auth logic
3. **Implement frontend** - UI components, state management
4. **Follow best practices** - See CLAUDE.md for code standards

### Important:

- Focus ONLY on code that satisfies the requirements
- Do NOT run tests or Stickler - that will be done automatically
- Do NOT create documentation files
- Follow the project's code standards in CLAUDE.md
- Ensure the implementation matches the user flow exactly

## Output

When complete, report:
- Files created/modified
- Brief description of implementation
- Any edge cases or assumptions
`.trim();
}

function buildFixAgentPrompt(
  runDir: string,
  iteration: number,
  previousAttempts: IterationRecord[],
  prd: ParsedPRD
): string {
  const recentFailures = previousAttempts
    .filter(a => a.status === 'failed')
    .slice(-3);

  return `
# Task: Fix Stickler Test Failure

You are a specialized agent that analyzes Stickler test failures and fixes code.

## Context

**Feature**: ${prd.name}
**Iteration**: ${iteration}
**Test Run Directory**: ${runDir}

## Previous Attempts

${recentFailures.map((f, i) => `
### Attempt ${f.iteration}
- Phase: ${f.phase}
- Error: ${f.error || 'Unknown'}
`).join('\n')}

## Test Failure Analysis

1. **Review test artifacts**:
   - Check screenshots in ${runDir}/
   - Read planning_request.json to understand what Stickler was trying to do
   - Review the test output

2. **Identify root cause**:
   - Is the UI element missing?
   - Is the text/selector wrong?
   - Is there a navigation issue?
   - Is the backend not responding correctly?

3. **Apply targeted fix**:
   - Fix ONLY the specific issue causing the failure
   - Do NOT refactor or add features
   - Ensure fix aligns with PRD requirements

## Your Task

1. Analyze the failure in ${runDir}
2. Determine the root cause
3. Apply a minimal, targeted fix
4. Report what you fixed and why

## Important

- Do NOT run Stickler again - that will be done automatically
- Focus on the SPECIFIC failure, not general improvements
- Review previous attempts to avoid repeating the same fix

## Output

When complete, report:
- Root cause of failure
- Files modified
- Description of fix
- Confidence level (high/medium/low)
`.trim();
}
