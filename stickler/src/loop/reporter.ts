/**
 * Progress Reporter - Format loop progress and reports
 */

import { LoopState, LoopReport, IterationRecord } from './types.js';

export function logProgress(state: LoopState, message: string, verbose: boolean = false): void {
  if (!verbose && message.includes('[DEBUG]')) {
    return;
  }

  const prefix = `[Loop ${state.currentIteration}/${state.totalIterations}]`;
  console.log(`${prefix} ${message}`);
}

export function formatLoopReport(report: LoopReport): string {
  const lines: string[] = [];
  const duration = (report.totalDuration / 1000).toFixed(1);

  lines.push('');
  lines.push('═══════════════════════════════════════════════════════');
  lines.push('                    DEV LOOP REPORT                     ');
  lines.push('═══════════════════════════════════════════════════════');
  lines.push('');
  lines.push(`PRD: ${report.prdName}`);
  lines.push(`Status: ${formatStatus(report.status)}`);
  lines.push(`Iterations: ${report.totalIterations}`);
  lines.push(`Duration: ${duration}s`);
  lines.push('');

  if (report.scenarioPath) {
    lines.push(`Scenario: ${report.scenarioPath}`);
    lines.push('');
  }

  if (report.status === 'stuck' && report.stuckReason) {
    lines.push('🔴 STUCK');
    lines.push(`Reason: ${report.stuckReason}`);
    lines.push('');
  }

  lines.push('Iteration History:');
  lines.push(generateIterationHistory(report.iterations));
  lines.push('');

  lines.push(report.finalMessage);
  lines.push('');
  lines.push('═══════════════════════════════════════════════════════');

  return lines.join('\n');
}

export function generateIterationHistory(iterations: IterationRecord[]): string {
  const lines: string[] = [];

  for (const iter of iterations) {
    const icon = iter.status === 'success' ? '✅' : '❌';
    const phase = formatPhase(iter.phase);
    const duration = (iter.duration / 1000).toFixed(1);

    lines.push(`  ${icon} [${iter.iteration}] ${phase} (${duration}s)`);

    if (iter.error) {
      const errorPreview = iter.error.substring(0, 80);
      lines.push(`     └─ ${errorPreview}${iter.error.length > 80 ? '...' : ''}`);
    }
  }

  return lines.join('\n');
}

function formatStatus(status: string): string {
  switch (status) {
    case 'success':
      return '✅ SUCCESS';
    case 'failed':
      return '❌ FAILED';
    case 'stuck':
      return '🔴 STUCK';
    default:
      return status.toUpperCase();
  }
}

function formatPhase(phase: IterationRecord['phase']): string {
  switch (phase) {
    case 'scenario_gen':
      return 'Generate Scenario';
    case 'code_impl':
      return 'Implement Code';
    case 'test':
      return 'Run Test';
    case 'fix':
      return 'Apply Fix';
    default:
      return phase;
  }
}

export function logIterationStart(iteration: number, phase: string): void {
  console.log(`\n🔄 Starting iteration ${iteration}: ${phase}`);
}

export function logIterationComplete(iteration: number, success: boolean, duration: number): void {
  const icon = success ? '✅' : '❌';
  const status = success ? 'SUCCESS' : 'FAILED';
  const time = (duration / 1000).toFixed(1);
  console.log(`${icon} Iteration ${iteration} ${status} (${time}s)`);
}
