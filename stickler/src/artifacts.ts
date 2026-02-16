import { writeFile, appendFile } from 'fs/promises';
import { join } from 'path';
import { Verdict, Issue, TraceEntry } from './ui/types.js';

/**
 * Save the final verdict of a test run
 */
export async function saveVerdict(verdict: Verdict, runDir: string): Promise<void> {
  const verdictPath = join(runDir, 'verdict.json');
  await writeFile(verdictPath, JSON.stringify(verdict, null, 2));
  console.log(`\n📄 Verdict saved to: ${verdictPath}`);
}

/**
 * Save an issue file when a test fails
 */
export async function saveIssue(issue: Issue, runDir: string): Promise<void> {
  const issuePath = join(runDir, 'issue.json');
  await writeFile(issuePath, JSON.stringify(issue, null, 2));
  console.log(`\n🐛 Issue file saved to: ${issuePath}`);

  // Also generate a fix.md file with a prompt for Claude Code
  const fixPrompt = generateFixPrompt(issue);
  const fixPath = join(runDir, 'fix.md');
  await writeFile(fixPath, fixPrompt);
  console.log(`📝 Fix prompt saved to: ${fixPath}`);
}

/**
 * Save/update the trace log
 */
export async function saveTrace(trace: TraceEntry[], runDir: string): Promise<void> {
  const tracePath = join(runDir, 'trace.jsonl');

  // Write entire trace (overwrite mode)
  const content = trace.map((entry) => JSON.stringify(entry)).join('\n') + '\n';
  await writeFile(tracePath, content);
}

/**
 * Generate a markdown prompt for Claude Code to fix the issue
 */
function generateFixPrompt(issue: Issue): string {
  return `# Stickler Test Failure - Fix Needed

## Scenario
**Name**: ${issue.scenario}
**Timestamp**: ${issue.timestamp}

## What Went Wrong
${issue.context_for_claude.what_went_wrong}

## What Was Expected
${issue.context_for_claude.what_was_expected}

## Failure Details

### Last Action Attempted
\`\`\`json
${JSON.stringify(issue.last_action, null, 2)}
\`\`\`

### UI State at Failure
- **URL**: ${issue.context_for_claude.ui_state_at_failure.url}
- **Visible Elements**:
${issue.context_for_claude.ui_state_at_failure.visible_elements.map((el) => `  - ${el}`).join('\n')}

### Visible Errors
${issue.visible_errors.length > 0 ? issue.visible_errors.map((err) => `- ${err}`).join('\n') : '_No error messages detected in UI_'}

## Evidence
- **Screenshot**: [${issue.final_screenshot}](${issue.final_screenshot})
- **Full Trace**: [${issue.full_trace_path}](${issue.full_trace_path})

## Suggested Fixes
${issue.suggested_fixes.map((fix, idx) => `${idx + 1}. ${fix}`).join('\n')}

---

## Instructions for Claude Code

Please review the evidence above and fix the issue. Here's what to do:

1. **Read the screenshot** using the Read tool to see the exact UI state
2. **Review the trace** to understand the sequence of actions
3. **Identify the root cause** of the failure
4. **Implement the fix** in the codebase
5. **Run Stickler again** to verify the fix works

Once you've made changes, I (Stickler) can re-test the scenario to verify the fix.
`;
}
