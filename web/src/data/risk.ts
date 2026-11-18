/**
 * A plain check (not a model) for commands an agent asks to run that are hard
 * to undo (design B3, example 3). When one matches, Sentinal says what it
 * does and never offers "don't ask again" for it.
 */
const RULES: Array<[RegExp, string]> = [
  [/\brm\s+(-[a-z]*r[a-z]*f?|-[a-z]*f[a-z]*r)\b|\brm\s+-r\b/i, "deletes files"],
  [/\bgit\s+push\b[^\n]*\s(--force\b|-f\b|--force-with-lease\b)/i, "rewrites the remote branch"],
  [/\bgit\s+reset\s+--hard\b/i, "throws away local changes"],
  [/\bgit\s+clean\s+-[a-z]*f/i, "deletes untracked files"],
  [/\bdrop\s+(table|database|schema)\b/i, "drops a database table"],
  [/\btruncate\s+table\b/i, "empties a database table"],
  [/\b(mkfs|dd\s+if=)/i, "writes over a disk"],
  [/\bchmod\s+-R\s+777\b/i, "opens every file to everyone"],
  [/\b(curl|wget)\b[^\n|]*\|\s*(sudo\s+)?(ba|z)?sh\b/i, "runs a downloaded script"],
  [/\bsudo\b/i, "runs as administrator"],
];

/** What a risky command does, or null when nothing matched. */
export function riskOf(text: string | undefined): string | null {
  if (!text) return null;
  for (const [pattern, label] of RULES) if (pattern.test(text)) return label;
  return null;
}

/** "Yes, and don't ask again …", "Always allow": blanket answers. */
export function isBlanketAnswer(label: string): boolean {
  return /don.?t ask again|always (allow|approve|run)|allow all/i.test(label);
}
