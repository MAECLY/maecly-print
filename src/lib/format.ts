/**
 * Format a size in kibibytes into a human-friendly string.
 * Ported from `claude design/maeclyprint/project/data.jsx:57`.
 */
export function fmtKb(kb: number): string {
  if (kb < 1024) return `${kb} KB`;
  return `${(kb / 1024).toFixed(1)} MB`;
}

/**
 * Pluralize a count by appending "s" when not 1.
 */
export function pluralize(n: number, singular: string, plural?: string) {
  return n === 1 ? singular : (plural ?? `${singular}s`);
}
