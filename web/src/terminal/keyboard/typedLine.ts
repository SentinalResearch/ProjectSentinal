/**
 * The line being typed on the built-in keyboard, so the phone can offer
 * completions for it (Milestone 5.A). It follows only what this keyboard
 * sent: `null` means "unknown" once a key moved the cursor, recalled
 * history, or completed with Tab, until Enter starts a fresh line.
 */
export function nextTypedLine(line: string | null, bytes: string): string | null {
  // Enter, ^C, and ^U start a fresh line.
  if (bytes === "\r" || bytes === "\x03" || bytes === "\x15") return "";
  // ^L redraws the screen without touching the line.
  if (bytes === "\x0c") return line;
  if (line === null) return null;
  if (bytes === "\x7f") return Array.from(line).slice(0, -1).join("");
  // Escape sequences, Tab, and other control keys: the line is no longer known.
  if (/[\x00-\x1f]/.test(bytes)) return null;
  return line + bytes;
}
