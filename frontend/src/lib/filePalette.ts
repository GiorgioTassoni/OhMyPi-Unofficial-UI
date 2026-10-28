/**
 * Model and token helpers for `@` file mentions in the composer.
 */
export interface AtToken {
  /** The 0-based offset of the `@` in the draft. */
  start: number;
  /** What has been typed after the `@` up to the caret. */
  query: string;
}

/**
 * Finds an active `@` file token before the caret, or `null` when no mention is active.
 *
 * Rules:
 * - The `@` must be at the start of the draft, or preceded by whitespace or common punctuation (`(`, `[`, `{`, `'`, `"`, `,`, `:`).
 * - The text after `@` must not contain any whitespace.
 */
export function findAtToken(before: string): AtToken | null {
  const lastAt = before.lastIndexOf("@");
  if (lastAt === -1) {
    return null;
  }

  // If preceded by a character, it must be whitespace or delimiter punctuation
  if (lastAt > 0) {
    const prevChar = before[lastAt - 1];
    if (!/[\s(\[{'",:]/.test(prevChar)) {
      return null;
    }
  }

  const afterAt = before.slice(lastAt + 1);

  // If there is any whitespace after '@', the user has completed typing the mention token.
  if (/\s/.test(afterAt)) {
    return null;
  }

  return {
    start: lastAt,
    query: afterAt,
  };
}
