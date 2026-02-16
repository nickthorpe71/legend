/**
 * PRD (Product Requirements Document) types
 */

export interface ParsedPRD {
  name: string;
  context: string;
  requirements: string[];
  userFlow: string[];
  acceptanceCriteria: string[];
  technicalNotes: string[];
  testData: Record<string, string>;
  successSignals: string[];
  failureSignals: string[];
  rawContent: string;
}

export interface PRDValidationResult {
  valid: boolean;
  errors: string[];
  warnings: string[];
}

export interface PRDMetadata {
  filePath: string;
  name: string;
  createdAt?: Date;
  lastModified?: Date;
}
