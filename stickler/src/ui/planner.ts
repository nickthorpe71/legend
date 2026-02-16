import { writeFile, readFile, unlink } from 'fs/promises';
import { join } from 'path';
import { Action, ActionSchema, UIState, Scenario } from './types.js';

interface PlannerContext {
  uiState: UIState;
  scenario: Scenario;
  actionHistory: Action[];
  stepNumber: number;
  runDir: string; // Directory for this test run
}

/**
 * Planner that uses Claude Code (running in the current session) to decide next actions.
 *
 * This creates a planning request file that Claude Code can read and respond to.
 * The user will need to ask Claude Code to review the request and provide an action.
 */
export async function planNextAction(context: PlannerContext): Promise<Action> {
  const { uiState, scenario, actionHistory, stepNumber, runDir } = context;

  // Note: Success/fail verification is handled by the verifier in the main loop
  // The planner just decides the next action

  // Compute form state for quick reference
  const filledInputs = uiState.interactables
    .filter((el) => el.type === 'input' && el.value && el.value.length > 0)
    .map((el) => el.label || el.placeholder);
  const emptyInputs = uiState.interactables
    .filter((el) => el.type === 'input' && (!el.value || el.value.length === 0))
    .map((el) => el.label || el.placeholder);
  const availableButtons = uiState.interactables.filter((el) => el.type === 'button').map((el) => el.label);

  // Create planning request for Claude Code
  const planningRequest = {
    step: stepNumber,
    timestamp: new Date().toISOString(),
    scenario: {
      name: scenario.name,
      objective: scenario.objective,
      success_signals: scenario.success_signals,
      fail_signals: scenario.fail_signals,
      context: scenario.context,
      test_data: scenario.test_data,
    },
    current_state: {
      url: uiState.url,
      screenshot_path: join(runDir, 'screenshots', `step_${String(stepNumber).padStart(3, '0')}.png`),
      visible_text: uiState.visibleText.slice(0, 30), // Limit for readability
      interactables: uiState.interactables.map((el, idx) => ({
        index: idx + 1,
        type: el.type,
        label: el.label,
        placeholder: el.placeholder,
        value: el.value,
      })),
      // Summary of filled vs empty inputs for quick reference
      form_state: {
        filled_inputs: filledInputs,
        empty_inputs: emptyInputs,
        available_buttons: availableButtons,
      },
    },
    action_history: actionHistory.slice(-5).map((a, idx) => ({
      step: stepNumber - (actionHistory.slice(-5).length - idx),
      action: a,
    })),
    instructions: `
# Planning Request for Claude Code

Claude Code, please analyze the current UI state and decide the next action.

## Scenario
- **Objective**: ${scenario.objective}
- **Success Signals**: ${scenario.success_signals.join(', ')}
- **Fail Signals**: ${scenario.fail_signals.join(', ')}
${scenario.context ? `- **Context**: ${scenario.context}` : ''}

## Current State
- **URL**: ${uiState.url}
- **Screenshot**: See ${join(runDir, 'screenshots', `step_${String(stepNumber).padStart(3, '0')}.png`)}
- **Visible Text**: ${uiState.visibleText.slice(0, 10).join(' | ')}
- **Interactables**: ${uiState.interactables.length} elements found

## Form State (Quick Reference)
- **Filled Inputs**: ${filledInputs.length > 0 ? filledInputs.join(', ') : 'None'}
- **Empty Inputs**: ${emptyInputs.length > 0 ? emptyInputs.join(', ') : 'None'}
- **Available Buttons**: ${availableButtons.length > 0 ? availableButtons.join(', ') : 'None'}

## Available Actions
Respond with ONE action in JSON format:

1. **Click**: { "type": "click", "target": "<exact label>", "reason": "<why>" }
2. **Type**: { "type": "type", "target": "<input label>", "text": "<text>", "reason": "<why>" }
3. **Scroll**: { "type": "scroll", "direction": "up|down", "amount": 300, "reason": "<why>" }
4. **Wait**: { "type": "wait", "duration": 1000, "reason": "<why>" }
5. **Done**: { "type": "done", "reason": "<why complete>" }
6. **Stuck**: { "type": "stuck", "reason": "<why stuck>" }

## Decision-Making Guidelines
1. **Review the screenshot** to see the exact UI state
2. **Check Form State** - If inputs are filled but empty inputs remain invisible, look for a button to progress (common in multi-step forms)
3. **Check Available Buttons** - If form fields are filled, there's likely a submit/continue button to click
4. **Check interactables list** for exact element labels (use these exact labels in your action)
5. **Consider action history** to avoid loops
6. **Write your response** to: ${join(runDir, 'planning_response.json')}

Example response format:
{
  "type": "click",
  "target": "Sign In",
  "reason": "Need to click the login button after filling credentials"
}
`.trim(),
  };

  // Save planning request
  const requestPath = join(runDir, 'planning_request.json');
  await writeFile(requestPath, JSON.stringify(planningRequest, null, 2));

  // Save screenshot with step number
  const screenshotPath = join(runDir, 'screenshots', `step_${String(stepNumber).padStart(3, '0')}.png`);
  await writeFile(screenshotPath, uiState.screenshot);

  console.log('\n' + '='.repeat(80));
  console.log('⏸️  PLANNING REQUIRED');
  console.log('='.repeat(80));
  console.log(`\n📋 Planning request created at:`);
  console.log(`   ${requestPath}`);
  console.log(`\n📸 Screenshot saved at:`);
  console.log(`   ${screenshotPath}`);
  console.log(`\n💬 Please ask Claude Code:`);
  console.log(`   "Review the planning request and screenshot, then provide the next action"`);
  console.log(`\n✍️  Claude Code should write response to:`);
  console.log(`   ${join(runDir, 'planning_response.json')}`);
  console.log('\n' + '='.repeat(80));

  // Wait for planning response
  const responsePath = join(runDir, 'planning_response.json');
  const action = await waitForPlanningResponse(responsePath);

  return action;
}

/**
 * Wait for Claude Code to provide a planning response
 */
async function waitForPlanningResponse(responsePath: string): Promise<Action> {
  const maxWaitTime = 300000; // 5 minutes
  const pollInterval = 500; // 0.5 seconds
  const startTime = Date.now();

  while (Date.now() - startTime < maxWaitTime) {
    try {
      const content = await readFile(responsePath, 'utf-8');
      const data = JSON.parse(content);
      const action = ActionSchema.parse(data);

      // Delete the response file so it's not reused on next step
      await unlink(responsePath).catch(() => {
        // Ignore errors if file already deleted
      });

      console.log(`\n✅ Received action from Claude Code: ${action.type}`);
      return action;
    } catch (error) {
      // File doesn't exist yet or invalid JSON - keep waiting
      await new Promise((resolve) => setTimeout(resolve, pollInterval));
    }
  }

  throw new Error('Timeout waiting for planning response from Claude Code');
}
