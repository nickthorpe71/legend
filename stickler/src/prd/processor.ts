/**
 * PRD Processor - Parse markdown PRD files into structured format
 */

import * as fs from 'fs';
import * as path from 'path';
import { ParsedPRD } from './types.js';

export function parsePRD(prdPath: string): ParsedPRD {
  if (!fs.existsSync(prdPath)) {
    throw new Error(`PRD file not found: ${prdPath}`);
  }

  const content = fs.readFileSync(prdPath, 'utf-8');
  const name = path.basename(prdPath, '.md');

  // Parse markdown sections
  const sections = parseMarkdownSections(content);

  return {
    name,
    context: sections.context || '',
    requirements: parseListItems(sections.requirements || ''),
    userFlow: parseListItems(sections['user flow'] || ''),
    acceptanceCriteria: parseListItems(sections['acceptance criteria'] || ''),
    technicalNotes: parseListItems(sections['technical notes'] || ''),
    testData: parseTestData(sections['test data'] || ''),
    successSignals: parseListItems(sections['success signals'] || ''),
    failureSignals: parseListItems(sections['failure signals'] || ''),
    rawContent: content,
  };
}

function parseMarkdownSections(content: string): Record<string, string> {
  const sections: Record<string, string> = {};
  const lines = content.split('\n');

  let currentSection = '';
  let currentContent: string[] = [];

  for (const line of lines) {
    // Check for ## heading
    const headingMatch = line.match(/^##\s+(.+)$/);
    if (headingMatch) {
      // Save previous section
      if (currentSection) {
        sections[currentSection.toLowerCase()] = currentContent.join('\n').trim();
      }
      currentSection = headingMatch[1];
      currentContent = [];
    } else {
      currentContent.push(line);
    }
  }

  // Save last section
  if (currentSection) {
    sections[currentSection.toLowerCase()] = currentContent.join('\n').trim();
  }

  return sections;
}

function parseListItems(content: string): string[] {
  return content
    .split('\n')
    .filter(line => line.trim().match(/^[-*]\s+/))
    .map(line => line.replace(/^[-*]\s+/, '').trim())
    .filter(item => item.length > 0);
}

function parseTestData(content: string): Record<string, string> {
  const data: Record<string, string> = {};
  const lines = content.split('\n');

  for (const line of lines) {
    // Match format: - key: value
    const match = line.match(/^[-*]\s+(\w+):\s*(.+)$/);
    if (match) {
      data[match[1]] = match[2].trim();
    }
  }

  return data;
}
