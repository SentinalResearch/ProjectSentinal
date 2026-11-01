import type { Terminal } from "@xterm/xterm";

/** Words not worth a chip. */
const COMMON = new Set(["the", "and", "for", "with", "from", "this", "that", "are", "was", "you", "not", "but", "all", "can"]);

/** Up to `limit` frequent words (3+ letters) from the last lines of `text`, most frequent first. */
export function frequentTokens(text: string, limit = 6): string[] {
  const counts = new Map<string, number>();
  for (const word of text.match(/[A-Za-z][A-Za-z_]{2,}/g) ?? []) {
    if (COMMON.has(word.toLowerCase())) continue;
    counts.set(word, (counts.get(word) ?? 0) + 1);
  }
  return [...counts.entries()]
    .sort((left, right) => right[1] - left[1] || left[0].localeCompare(right[0]))
    .slice(0, limit)
    .map(([word]) => word);
}

/** Tokens from the terminal's last 40 lines (MOBILE_GLASS_SPEC §4: "tokens from recent output"). */
export function outputTokens(terminal: Terminal, limit = 6): string[] {
  const buffer = terminal.buffer.active;
  const end = buffer.baseY + buffer.cursorY;
  const lines: string[] = [];
  for (let row = Math.max(0, end - 40); row <= end; row += 1) {
    lines.push(buffer.getLine(row)?.translateToString(true) ?? "");
  }
  return frequentTokens(lines.join("\n"), limit);
}
