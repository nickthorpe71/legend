/**
 * Dev Loop Types
 */

export interface LoopConfig {
  prdPath: string;
  maxIterations: number;
  maxSameError: number;
  verbose: boolean;
  workspaceRoot: string;
}

export interface LoopState {
  prdName: string;
  currentIteration: number;
  totalIterations: number;
  status: 'running' | 'success' | 'failed' | 'stuck';
  iterations: IterationRecord[];
  scenarioPath: string;
  startTime: number;
}

export interface IterationRecord {
  iteration: number;
  phase: 'scenario_gen' | 'code_impl' | 'test' | 'fix';
  status: 'success' | 'failed';
  duration: number;
  error?: string;
  errorHash?: string;
  details: string;
  timestamp: number;
}

export interface LoopReport {
  prdName: string;
  status: 'success' | 'failed' | 'stuck';
  totalIterations: number;
  totalDuration: number;
  iterations: IterationRecord[];
  finalMessage: string;
  stuckReason?: string;
  scenarioPath?: string;
}

export interface StuckDetectionResult {
  stuck: boolean;
  reason: string;
  repeatedError?: string;
  count?: number;
}
