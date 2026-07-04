import type { LineRecord } from '@/api/types';

export function normalizeTextFilterPatterns(patterns: readonly string[]): string[] {
  return patterns.filter((pattern) => pattern.length > 0);
}

export function validateTextFilterPatterns(patterns: readonly string[]): Array<string | null> {
  return patterns.map((pattern) => {
    if (!pattern) {
      return null;
    }

    try {
      new RegExp(pattern, 'g');
      return null;
    } catch (error) {
      return error instanceof Error ? error.message : 'Invalid regex';
    }
  });
}

export function hasTextFilterErrors(errors: readonly (string | null)[]): boolean {
  return errors.some(Boolean);
}

export function filterLineRecords(
  lines: readonly LineRecord[],
  patterns: readonly string[],
): LineRecord[] {
  const filters = compileTextFilters(patterns);
  return lines
    .map((line) => ({
      ...line,
      text: applyCompiledTextFilters(line.text, filters),
    }))
    .filter((line) => line.text.trim().length > 0);
}

export function buildFilteredSentence(lines: readonly LineRecord[], separator: string): string {
  return lines
    .map((line) => line.text.trim())
    .filter((text) => text.length > 0)
    .join(separator);
}

function compileTextFilters(patterns: readonly string[]): RegExp[] {
  const filters: RegExp[] = [];
  for (const pattern of normalizeTextFilterPatterns(patterns)) {
    try {
      filters.push(new RegExp(pattern, 'g'));
    } catch {
      // Stored settings can be edited outside the UI; ignore invalid saved filters at runtime.
    }
  }
  return filters;
}

function applyCompiledTextFilters(text: string, filters: readonly RegExp[]): string {
  let next = text;
  for (const filter of filters) {
    filter.lastIndex = 0;
    next = next.replace(filter, '');
  }
  return next;
}
