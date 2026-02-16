import { chromium, Browser, Page } from 'playwright';
import { mkdir } from 'fs/promises';
import { join } from 'path';
import { Scenario, TraceEntry, Action, Config } from './ui/types.js';
import { captureUIState } from './ui/observer.js';
import { planNextAction } from './ui/planner.js';
import { executeAction } from './ui/actor.js';
import { verifyObjective } from './ui/verifier.js';
import { saveVerdict, saveIssue, saveTrace } from './artifacts.js';

interface RunResult {
  status: 'PASS' | 'FAIL' | 'WARN';
  duration_ms: number;
  steps_taken: number;
  runDir: string;
}

export async function runScenario(scenario: Scenario, config: Config): Promise<RunResult> {
  const startTime = Date.now();
  const runDir = await createRunDirectory(scenario.name);

  console.log(`\n${'='.repeat(80)}`);
  console.log(`🎯 Running scenario: ${scenario.name}`);
  console.log(`📂 Run directory: ${runDir}`);
  console.log(`${'='.repeat(80)}\n`);

  let browser: Browser | undefined;
  let page: Page | undefined;

  try {
    // Launch browser
    browser = await chromium.launch({
      headless: true, // Run headless for WSL compatibility
    });

    const context = await browser.newContext({
      viewport: config.viewport,
    });

    page = await context.newPage();

    // Navigate to start URL
    console.log(`🌐 Navigating to: ${scenario.start_url}`);
    await page.goto(scenario.start_url, { waitUntil: 'networkidle' });

    // Main loop
    const result = await runMainLoop(page, scenario, config, runDir);

    // Generate verdict
    const duration = Date.now() - startTime;
    await saveVerdict({
      scenario: scenario.name,
      status: result.status,
      duration_ms: duration,
      steps_taken: result.steps_taken,
      final_url: page.url(),
      reason: result.reason,
      timestamp: new Date().toISOString(),
    }, runDir);

    // Generate issue if failed
    if (result.status === 'FAIL' && result.lastAction && result.finalState) {
      await saveIssue({
        scenario: scenario.name,
        timestamp: new Date().toISOString(),
        failure_reason: result.reason,
        last_action: result.lastAction,
        final_screenshot: join(runDir, 'screenshots', 'final.png'),
        visible_errors: result.finalState.visibleText.filter((t) =>
          /error|fail|invalid|incorrect/i.test(t)
        ),
        suggested_fixes: result.suggestedFixes || [],
        full_trace_path: join(runDir, 'trace.jsonl'),
        context_for_claude: {
          what_went_wrong: result.reason,
          what_was_expected: scenario.objective,
          ui_state_at_failure: {
            url: result.finalState.url,
            visible_elements: result.finalState.interactables.map((el) => `${el.type}: ${el.label}`),
          },
        },
      }, runDir);
    }

    console.log(`\n${'='.repeat(80)}`);
    console.log(`${result.status === 'PASS' ? '✅' : '❌'} Result: ${result.status}`);
    console.log(`⏱️  Duration: ${duration}ms`);
    console.log(`📊 Steps taken: ${result.steps_taken}`);
    console.log(`${'='.repeat(80)}\n`);

    return {
      status: result.status,
      duration_ms: duration,
      steps_taken: result.steps_taken,
      runDir,
    };
  } finally {
    await browser?.close();
  }
}

async function runMainLoop(page: Page, scenario: Scenario, config: Config, runDir: string) {
  const trace: TraceEntry[] = [];
  const actionHistory: Action[] = [];
  const maxSteps = scenario.max_steps || config.maxSteps;

  for (let step = 0; step < maxSteps; step++) {
    console.log(`\n--- Step ${step + 1}/${maxSteps} ---`);

    // 1. OBSERVE
    console.log('👀 Observing UI state...');
    const uiState = await captureUIState(page);
    console.log(`   URL: ${uiState.url}`);
    console.log(`   Interactables: ${uiState.interactables.length}`);
    console.log(`   Visible text: ${uiState.visibleText.length} snippets`);

    // 2. VERIFY
    console.log('🔍 Verifying objective...');
    const verification = await verifyObjective(uiState, scenario);
    console.log(`   Status: ${verification.status}`);
    console.log(`   Reason: ${verification.reason}`);

    if (verification.status === 'success') {
      return {
        status: 'PASS' as const,
        reason: verification.reason,
        steps_taken: step + 1,
      };
    }

    if (verification.status === 'failure') {
      return {
        status: 'FAIL' as const,
        reason: verification.reason,
        steps_taken: step + 1,
        lastAction: actionHistory[actionHistory.length - 1],
        finalState: uiState,
        suggestedFixes: [
          'Check if the fail signals in the scenario are correct',
          'Review the last action to see if it caused an error',
          'Check the final screenshot for UI errors',
        ],
      };
    }

    // 3. PLAN
    console.log('🧠 Planning next action...');
    const action = await planNextAction({
      uiState,
      scenario,
      actionHistory,
      stepNumber: step + 1,
      runDir,
    });
    console.log(`   Action: ${action.type}`);
    console.log(`   Reason: ${action.reason}`);

    // Check for terminal actions
    if (action.type === 'done') {
      return {
        status: 'PASS' as const,
        reason: action.reason,
        steps_taken: step + 1,
      };
    }

    if (action.type === 'stuck') {
      return {
        status: 'FAIL' as const,
        reason: action.reason,
        steps_taken: step + 1,
        lastAction: action,
        finalState: uiState,
        suggestedFixes: [
          'Review the planning request to see why Stickler got stuck',
          'Check if the scenario signals are accurate',
          'Look at the screenshot to see if there are unexpected UI elements',
        ],
      };
    }

    // 4. ACT
    console.log('🎬 Executing action...');
    const executionResult = await executeAction(page, action, uiState, config);

    if (!executionResult.success) {
      console.log(`   ❌ Execution failed: ${executionResult.error}`);
      return {
        status: 'FAIL' as const,
        reason: `Action execution failed: ${executionResult.error}`,
        steps_taken: step + 1,
        lastAction: action,
        finalState: uiState,
        suggestedFixes: [
          `Fix the error: ${executionResult.error}`,
          'Check if the target element label is correct',
          'Review the screenshot to verify element visibility',
        ],
      };
    }

    console.log(`   ✅ Action executed successfully`);

    // Save trace entry
    const traceEntry: TraceEntry = {
      step: step + 1,
      timestamp: Date.now(),
      action,
      ui_state_before: {
        url: uiState.url,
        visible_text: uiState.visibleText.slice(0, 10),
        interactables_count: uiState.interactables.length,
      },
      execution_result: 'success',
    };
    trace.push(traceEntry);
    actionHistory.push(action);

    // Save trace periodically
    await saveTrace(trace, runDir);
  }

  // Max steps exceeded
  return {
    status: 'FAIL' as const,
    reason: `Maximum steps (${maxSteps}) exceeded without achieving objective`,
    steps_taken: maxSteps,
    lastAction: actionHistory[actionHistory.length - 1],
    finalState: await captureUIState(page),
    suggestedFixes: [
      'Increase max_steps in the scenario',
      'Review the trace to see where Stickler is getting stuck',
      'Simplify the objective or improve success signals',
    ],
  };
}

async function createRunDirectory(scenarioName: string): Promise<string> {
  const timestamp = new Date().toISOString().replace(/[:.]/g, '-').split('T').join('_').slice(0, -5);
  const runDir = join(process.cwd(), 'runs', `${timestamp}_${scenarioName}`);

  await mkdir(join(runDir, 'screenshots'), { recursive: true });
  await mkdir(join(runDir, 'logs'), { recursive: true });

  return runDir;
}
