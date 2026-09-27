export type AuthMode = "login" | "wrong" | "locked" | "expired" | "setup";

export interface AuthCopy {
  overline: string;
  title: string;
  body: string;
  overlineColor: string;
}

export const AUTH_COPY: Record<AuthMode, AuthCopy> = {
  login: {
    overline: "SIGN IN",
    title: "Welcome back",
    body: "Enter the admin password for this Sentinal server.",
    overlineColor: "var(--text-3)",
  },
  wrong: {
    overline: "SIGN IN",
    title: "Welcome back",
    body: "Enter the admin password for this Sentinal server.",
    overlineColor: "var(--text-3)",
  },
  locked: {
    overline: "LOCKED",
    title: "Too many attempts",
    body: "Sign-in is paused after 3 incorrect passwords. Your sessions keep running.",
    overlineColor: "var(--s-failed-fg)",
  },
  expired: {
    overline: "SESSION EXPIRED",
    title: "Sign in again",
    body: "Your sign-in on this browser expired. Your sessions kept running.",
    overlineColor: "var(--s-waiting-fg)",
  },
  setup: {
    overline: "FIRST RUN",
    title: "Create the admin password",
    body: "This server has no password yet. Sentinal has one account: yours. You'll use this password on every device.",
    overlineColor: "var(--accent-fg)",
  },
};
