import type { LineRecord } from '@/api/types';

export interface TextFilterOptions {
  regexes: readonly string[];
  deduplicateMultilinePrefixes: boolean;
}

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
  options: TextFilterOptions,
): LineRecord[] {
  const filters = compileTextFilters(options.regexes);
  return lines
    .map((line) => ({
      ...line,
      text: applyTextFilters(line.text, filters, options.deduplicateMultilinePrefixes),
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

function applyTextFilters(
  text: string,
  filters: readonly RegExp[],
  dedupeMultilinePrefixes: boolean,
): string {
  const filtered = applyCompiledTextFilters(text, filters);
  return dedupeMultilinePrefixes ? deduplicateMultilinePrefixes(filtered) : filtered;
}

export function deduplicateMultilinePrefixes(text: string): string {
  const physicalLines = text
    .replace(/\r\n?/g, '\n')
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
  const keptLines: string[] = [];

  for (const line of physicalLines) {
    const previous = keptLines.at(-1);
    if (previous && line.startsWith(previous)) {
      keptLines[keptLines.length - 1] = line;
    } else {
      keptLines.push(line);
    }
  }

  return keptLines.join('\n');
}
