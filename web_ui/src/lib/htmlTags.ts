export function preserveHtmlTags(existingValue: string, replacementText: string): string {
  const prefixTags = existingValue.match(/^(\s*(?:<[^>]+>\s*)*)/)?.[0] ?? '';
  const suffixTags = existingValue.match(/((?:\s*<\/[^>]+>)*\s*)$/)?.[0] ?? '';
  return `${prefixTags}${escapeHtml(replacementText)}${suffixTags}`;
}

export function stripHtml(value: string): string {
  return value.replace(/<[^>]*>/g, '').trim();
}

function escapeHtml(value: string): string {
  return value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;');
}
