/**
 * PRD Validator - Ensure PRD completeness
 */

import { ParsedPRD, PRDValidationResult } from './types.js';

export function validatePRD(prd: ParsedPRD): PRDValidationResult {
  const errors: string[] = [];
  const warnings: string[] = [];

  // Required sections
  if (!prd.name || prd.name.length === 0) {
    errors.push('PRD must have a name');
  }

  if (!prd.context || prd.context.length === 0) {
    warnings.push('PRD missing Context section');
  }

  if (prd.requirements.length === 0) {
    errors.push('PRD must have at least one Requirement');
  }

  if (prd.userFlow.length === 0) {
    warnings.push('PRD missing User Flow - recommended for better scenario generation');
  }

  if (prd.acceptanceCriteria.length === 0) {
    errors.push('PRD must have at least one Acceptance Criteria');
  }

  if (prd.successSignals.length === 0) {
    errors.push('PRD must have at least one Success Signal');
  }

  // Test data validation
  if (Object.keys(prd.testData).length === 0) {
    warnings.push('PRD has no Test Data - may need manual data entry');
  }

  // Warnings for missing optional sections
  if (prd.technicalNotes.length === 0) {
    warnings.push('PRD missing Technical Notes - may miss implementation details');
  }

  if (prd.failureSignals.length === 0) {
    warnings.push('PRD missing Failure Signals - may not detect errors properly');
  }

  return {
    valid: errors.length === 0,
    errors,
    warnings,
  };
}

export function formatValidationResult(result: PRDValidationResult): string {
  const lines: string[] = [];

  if (result.valid) {
    lines.push('✅ PRD validation passed');
  } else {
    lines.push('❌ PRD validation failed');
  }

  if (result.errors.length > 0) {
    lines.push('\nErrors:');
    result.errors.forEach(err => lines.push(`  - ${err}`));
  }

  if (result.warnings.length > 0) {
    lines.push('\nWarnings:');
    result.warnings.forEach(warn => lines.push(`  - ${warn}`));
  }

  return lines.join('\n');
}
