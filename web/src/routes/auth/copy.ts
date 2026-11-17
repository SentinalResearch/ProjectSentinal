import { config } from "../../data/config";

export type AuthMode = "startup" | "unreachable" | "login" | "wrong" | "locked" | "expired" | "setup";

export interface AuthCopy {
  overline: string;
  title: string;
  body: string;
  overlineColor: string;
}

const SIGN_IN: AuthCopy = {
  overline: "SIGN IN",
  title: "Welcome back",
  body: "Enter the admin password for this Sentinal server.",
  overlineColor: "var(--text-3)",
};

export const AUTH_COPY: Record<AuthMode, AuthCopy> = {
  startup: {
    overline: "STARTING",
    title: "Sentinal",
    body: "Control plane for your terminal sessions.",
    overlineColor: "var(--text-3)",
  },
  unreachable: {
    overline: "DISCONNECTED",
    title: "Can't reach the server",
    body: "Nothing answered at this address. Check that Sentinal is running, then try again.",
    overlineColor: "var(--text-3)",
  },
  login: SIGN_IN,
  wrong: SIGN_IN,
  locked: {
    overline: "LOCKED",
    title: "Too many attempts",
    body: `Sign-in is paused after ${config.loginMaxAttempts} incorrect passwords. Your sessions keep running.`,
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
