import { z } from 'zod';

// ===== Configuration Types =====

export const ConfigSchema = z.object({
  baseURL: z.string().url(),
  viewport: z.object({
    width: z.number(),
    height: z.number(),
  }),
  timeouts: z.object({
    pageLoad: z.number(),
    action: z.number(),
    maxRunDuration: z.number(),
  }),
  maxSteps: z.number(),
  actionDelay: z.object({
    min: z.number(),
    max: z.number(),
  }),
  llm: z.object({
    provider: z.enum(['anthropic', 'openai']),
    model: z.string(),
    maxTokens: z.number(),
    temperature: z.number(),
  }),
});

export type Config = z.infer<typeof ConfigSchema>;

// ===== Scenario Types =====

export const ScenarioSchema = z.object({
  name: z.string(),
  start_url: z.string(),
  objective: z.string(),
  success_signals: z.array(z.string()),
  fail_signals: z.array(z.string()),
  success_url_contains: z.array(z.string()).optional(), // URL must contain one of these
  fail_url_contains: z.array(z.string()).optional(), // URL must NOT contain any of these
  max_steps: z.number().optional(),
  context: z.string().optional(), // Additional context for the planner
  test_data: z.record(z.string(), z.string()).optional(), // Test data (credentials, form values, etc.)
});

export type Scenario = z.infer<typeof ScenarioSchema>;

// ===== UI State Types =====

export interface BoundingBox {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface InteractableElement {
  type: 'button' | 'link' | 'input' | 'select' | 'checkbox' | 'other';
  label: string; // Text content or aria-label
  role: string | null;
  boundingBox: BoundingBox;
  placeholder?: string; // For inputs
  value?: string; // Current value for inputs
}

export interface UIState {
  url: string;
  screenshot: Buffer;
  interactables: InteractableElement[];
  visibleText: string[]; // Headings, paragraphs, error messages
  timestamp: number;
}

// ===== Action Types =====

export const ActionSchema = z.discriminatedUnion('type', [
  z.object({
    type: z.literal('click'),
    target: z.string(), // Label of element to click
    reason: z.string(),
  }),
  z.object({
    type: z.literal('type'),
    target: z.string(), // Label of input to type into
    text: z.string(),
    reason: z.string(),
  }),
  z.object({
    type: z.literal('scroll'),
    direction: z.enum(['up', 'down']),
    amount: z.number().optional(), // pixels, default 300
    reason: z.string(),
  }),
  z.object({
    type: z.literal('wait'),
    duration: z.number(), // milliseconds
    reason: z.string(),
  }),
  z.object({
    type: z.literal('done'),
    reason: z.string(), // Why we think we're done
  }),
  z.object({
    type: z.literal('stuck'),
    reason: z.string(), // Why we're stuck
  }),
]);

export type Action = z.infer<typeof ActionSchema>;

// ===== Verification Types =====

export interface VerificationResult {
  status: 'success' | 'failure' | 'in_progress';
  reason: string;
  signals_found: string[]; // Which success/fail signals were detected
}

// ===== Trace Types =====

export interface TraceEntry {
  step: number;
  timestamp: number;
  action: Action;
  ui_state_before: {
    url: string;
    visible_text: string[];
    interactables_count: number;
  };
  execution_result: 'success' | 'error';
  error_message?: string;
}

// ===== Run Artifacts Types =====

export const VerdictSchema = z.object({
  scenario: z.string(),
  status: z.enum(['PASS', 'FAIL', 'WARN']),
  duration_ms: z.number(),
  steps_taken: z.number(),
  final_url: z.string(),
  reason: z.string(),
  timestamp: z.string(),
});

export type Verdict = z.infer<typeof VerdictSchema>;

export const IssueSchema = z.object({
  scenario: z.string(),
  timestamp: z.string(),
  failure_reason: z.string(),
  last_action: ActionSchema,
  final_screenshot: z.string(), // Path to screenshot
  visible_errors: z.array(z.string()),
  suggested_fixes: z.array(z.string()),
  full_trace_path: z.string(), // Path to trace.jsonl
  context_for_claude: z.object({
    what_went_wrong: z.string(),
    what_was_expected: z.string(),
    ui_state_at_failure: z.object({
      url: z.string(),
      visible_elements: z.array(z.string()),
    }),
  }),
});

export type Issue = z.infer<typeof IssueSchema>;
