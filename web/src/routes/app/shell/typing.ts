/** True when a key press is aimed at a text field or the terminal, so shell shortcuts must not fire. */
export function typingInto(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  if (target.closest(".xterm")) return true;
  return ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName);
}
