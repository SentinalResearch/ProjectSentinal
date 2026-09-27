/**
 * Values marked as assumptions in design_handoff_sentinal/fixtures.json → _assumptions.
 * They were invented by the designer to make the prototype work and need product
 * confirmation — kept here, not hard-coded, so they're easy to find and change.
 * See QUESTIONS.md.
 */
export const config = {
  pairCommand: "sentinal-agent pair <TOKEN>",
  tokenMask: "sa_••••xxxx",
  pairTokenTtlMs: 10 * 60 * 1000,
  loginMaxAttempts: 3,
  loginLockMs: 5 * 60 * 1000,
  setupMinPasswordLength: 12,
  historyRelativeDates: ["Today", "Yesterday", "3 days ago"],
} as const;
