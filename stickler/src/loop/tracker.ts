/**
 * Iteration Tracker - Record attempts and detect stuck states
 */

import * as crypto from 'crypto';
import { LoopState, IterationRecord, StuckDetectionResult } from './types.js';

export function recordIteration(
  state: LoopState,
  phase: IterationRecord['phase'],
  status: IterationRecord['status'],
  details: string,
  error?: string,
  startTime?: number
): LoopState {
  const duration = startTime ? Date.now() - startTime : 0;

  const record: IterationRecord = {
    iteration: state.currentIteration,
    phase,
    status,
    duration,
    details,
    timestamp: Date.now(),
  };

  if (error) {
    record.error = error;
    record.errorHash = hashError(error);
  }

  return {
    ...state,
    iterations: [...state.iterations, record],
  };
}

export function detectStuck(state: LoopState, maxSameError: number): StuckDetectionResult {
  if (state.iterations.length < maxSameError) {
    return { stuck: false, reason: '' };
  }

  // Get recent test failures
  const recentTests = state.iterations
    .filter(iter => iter.phase === 'test' && iter.status === 'failed')
    .slice(-maxSameError);

  if (recentTests.length < maxSameError) {
    return { stuck: false, reason: '' };
  }

  // Check if all recent test failures have the same error hash
  const errorHashes = recentTests.map(test => test.errorHash).filter(Boolean);

  if (errorHashes.length < maxSameError) {
    return { stuck: false, reason: '' };
  }

  const firstHash = errorHashes[0];
  const allSame = errorHashes.every(hash => hash === firstHash);

  if (allSame) {
    const repeatedError = recentTests[0].error || 'Unknown error';
    return {
      stuck: true,
      reason: `Same error repeated ${maxSameError} times`,
      repeatedError,
      count: maxSameError,
    };
  }

  return { stuck: false, reason: '' };
}

function hashError(error: string): string {
  // Normalize error message for comparison
  const normalized = error
    .toLowerCase()
    .replace(/\d+/g, 'N') // Replace numbers with N
    .replace(/line \d+/gi, 'line N')
    .replace(/at .+:\d+:\d+/g, 'at FILE:N:N')
    .trim();

  return crypto.createHash('md5').update(normalized).digest('hex');
}

export function getRecentFailures(state: LoopState, count: number = 3): IterationRecord[] {
  return state.iterations
    .filter(iter => iter.status === 'failed')
    .slice(-count);
}

export function getIterationSummary(state: LoopState): string {
  const total = state.iterations.length;
  const successes = state.iterations.filter(i => i.status === 'success').length;
  const failures = state.iterations.filter(i => i.status === 'failed').length;

  return `${total} total (${successes} success, ${failures} failed)`;
}
