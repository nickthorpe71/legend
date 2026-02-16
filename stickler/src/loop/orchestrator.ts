/**
 * Loop Orchestrator - Main dev loop state machine
 */

import * as path from 'path';
import { parsePRD } from '../prd/processor.js';
import { validatePRD, formatValidationResult } from '../prd/validator.js';
import { runScenario } from '../runner.js';
import { loadScenario, loadConfig } from '../scenario.js';
import {
  LoopConfig,
  LoopState,
  LoopReport,
} from './types.js';
import {
  recordIteration,
  detectStuck,
  getRecentFailures,
} from './tracker.js';
import {
  logProgress,
  formatLoopReport,
  logIterationStart,
  logIterationComplete,
} from './reporter.js';
import {
  launchPRDAgent,
  launchCodeAgent,
  launchFixAgent,
} from './agents.js';

export async function runDevLoop(config: LoopConfig): Promise<LoopReport> {
  console.log('🚀 Starting Development Loop');
  console.log(`PRD: ${config.prdPath}`);
  console.log(`Max Iterations: ${config.maxIterations}`);
  console.log('');

  // Parse and validate PRD
  const prd = parsePRD(config.prdPath);
  const validation = validatePRD(prd);

  console.log(formatValidationResult(validation));
  console.log('');

  if (!validation.valid) {
    return {
      prdName: prd.name,
      status: 'failed',
      totalIterations: 0,
      totalDuration: 0,
      iterations: [],
      finalMessage: '❌ PRD validation failed. Please fix errors and try again.',
    };
  }

  // Initialize loop state
  let state: LoopState = {
    prdName: prd.name,
    currentIteration: 0,
    totalIterations: config.maxIterations,
    status: 'running',
    iterations: [],
    scenarioPath: '',
    startTime: Date.now(),
  };

  try {
    // Phase 1: Generate Scenario
    state.currentIteration = 1;
    logIterationStart(1, 'Generate Scenario');
    const scenarioStartTime = Date.now();

    const scenariosDir = path.join(path.dirname(config.prdPath), '..', 'scenarios');
    const scenarioPrompt = await launchPRDAgent(config.prdPath, prd, scenariosDir);

    // For manual mode: Print prompt and wait for user to complete
    console.log('\n📋 PRD AGENT PROMPT:');
    console.log('═══════════════════════════════════════════════════════');
    console.log(scenarioPrompt);
    console.log('═══════════════════════════════════════════════════════');
    console.log('\n⏸️  MANUAL MODE: Please create the scenario file as described above.');
    console.log(`Expected path: ${scenariosDir}/${prd.name}.json`);
    console.log('\nPress Ctrl+C when done, then run the loop again to continue.\n');

    state.scenarioPath = path.join(scenariosDir, `${prd.name}.json`);

    state = recordIteration(
      state,
      'scenario_gen',
      'success',
      'Scenario generation prompt provided',
      undefined,
      scenarioStartTime
    );

    logIterationComplete(1, true, Date.now() - scenarioStartTime);

    // Phase 2: Implement Code
    state.currentIteration = 2;
    logIterationStart(2, 'Implement Code');
    const codeStartTime = Date.now();

    const codePrompt = await launchCodeAgent(prd, state.scenarioPath, config.workspaceRoot);

    console.log('\n📋 CODE AGENT PROMPT:');
    console.log('═══════════════════════════════════════════════════════');
    console.log(codePrompt);
    console.log('═══════════════════════════════════════════════════════');
    console.log('\n⏸️  MANUAL MODE: Please implement the feature as described above.');
    console.log('\nPress Ctrl+C when done, then run the loop again to continue.\n');

    state = recordIteration(
      state,
      'code_impl',
      'success',
      'Code implementation prompt provided',
      undefined,
      codeStartTime
    );

    logIterationComplete(2, true, Date.now() - codeStartTime);

    // Phase 3: Test Loop (iterations 3+)
    for (let i = 3; i <= config.maxIterations; i++) {
      state.currentIteration = i;
      logIterationStart(i, 'Test Feature');

      const testStartTime = Date.now();

      try {
        // Run Stickler test
        const scenario = await loadScenario(prd.name);
        const sticklConfig = await loadConfig();
        const result = await runScenario(scenario, sticklConfig);

        if (result.status === 'PASS') {
          state = recordIteration(
            state,
            'test',
            'success',
            `Test passed after ${i - 2} iterations`,
            undefined,
            testStartTime
          );

          logIterationComplete(i, true, Date.now() - testStartTime);

          // SUCCESS!
          const report: LoopReport = {
            prdName: prd.name,
            status: 'success',
            totalIterations: state.iterations.length,
            totalDuration: Date.now() - state.startTime,
            iterations: state.iterations,
            scenarioPath: state.scenarioPath,
            finalMessage: `✅ Feature implemented and tested successfully!\n\nScenario: ${state.scenarioPath}\nTest Run: ${result.runDir}`,
          };

          console.log(formatLoopReport(report));
          return report;
        } else {
          // Test failed
          const errorMsg = `Test failed (${result.status}). Check: ${result.runDir}`;

          state = recordIteration(
            state,
            'test',
            'failed',
            'Test execution failed',
            errorMsg,
            testStartTime
          );

          logIterationComplete(i, false, Date.now() - testStartTime);

          // Check if stuck
          const stuckResult = detectStuck(state, config.maxSameError);
          if (stuckResult.stuck) {
            const report: LoopReport = {
              prdName: prd.name,
              status: 'stuck',
              totalIterations: state.iterations.length,
              totalDuration: Date.now() - state.startTime,
              iterations: state.iterations,
              scenarioPath: state.scenarioPath,
              stuckReason: stuckResult.reason,
              finalMessage: `🔴 Loop stuck: ${stuckResult.reason}\n\nRepeated error:\n${stuckResult.repeatedError}`,
            };

            console.log(formatLoopReport(report));
            return report;
          }

          // Launch fix agent
          const recentFailures = getRecentFailures(state, 3);
          const fixPrompt = await launchFixAgent(
            result.runDir,
            i,
            recentFailures,
            prd
          );

          console.log('\n📋 FIX AGENT PROMPT:');
          console.log('═══════════════════════════════════════════════════════');
          console.log(fixPrompt);
          console.log('═══════════════════════════════════════════════════════');
          console.log('\n⏸️  MANUAL MODE: Please analyze the failure and apply fix.');
          console.log(`Test artifacts: ${result.runDir}`);
          console.log('\nPress Ctrl+C when done, then run the loop again to continue.\n');

          const fixStartTime = Date.now();
          state = recordIteration(
            state,
            'fix',
            'success',
            'Fix prompt provided',
            undefined,
            fixStartTime
          );

          // In manual mode, we stop here and let user apply fix
          const report: LoopReport = {
            prdName: prd.name,
            status: 'failed',
            totalIterations: state.iterations.length,
            totalDuration: Date.now() - state.startTime,
            iterations: state.iterations,
            scenarioPath: state.scenarioPath,
            finalMessage: `⏸️  Paused for manual fix. Run dev-loop again after applying fix.`,
          };

          console.log(formatLoopReport(report));
          return report;
        }
      } catch (error: any) {
        state = recordIteration(
          state,
          'test',
          'failed',
          'Test execution error',
          error.message,
          testStartTime
        );

        logIterationComplete(i, false, Date.now() - testStartTime);
      }
    }

    // Max iterations reached
    const report: LoopReport = {
      prdName: prd.name,
      status: 'failed',
      totalIterations: state.iterations.length,
      totalDuration: Date.now() - state.startTime,
      iterations: state.iterations,
      scenarioPath: state.scenarioPath,
      finalMessage: `❌ Max iterations (${config.maxIterations}) reached without success.`,
    };

    console.log(formatLoopReport(report));
    return report;
  } catch (error: any) {
    const report: LoopReport = {
      prdName: prd.name,
      status: 'failed',
      totalIterations: state.iterations.length,
      totalDuration: Date.now() - state.startTime,
      iterations: state.iterations,
      finalMessage: `❌ Loop error: ${error.message}`,
    };

    console.log(formatLoopReport(report));
    return report;
  }
}
