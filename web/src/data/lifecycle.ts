import { useSyncExternalStore } from "react";

import type { Session } from "./types";

/**
 * How a terminal ends and how you hear about it (design L1–L3): live → done →
 * hidden → deleted, plus an inbox of notifications with read state.
 *
 * Kept in this browser for now: the daemon has no read state, auto-clear
 * setting or pins yet, so auto-clear only runs while Sentinal is open here.
 * Hiding and deleting themselves go to the daemon as before.
 */

export type AutoClear = "never" | "1h" | "1d";

export const AUTO_CLEAR_MS: Record<AutoClear, number> = { never: Infinity, "1h": 3_600_000, "1d": 86_400_000 };
export const AUTO_CLEAR_LABEL: Record<AutoClear, string> = { never: "never", "1h": "after 1 hour", "1d": "after 1 day" };

export interface LifecycleState {
  autoClear: AutoClear;
  /** Sessions pinned with "Keep forever": auto-clear skips them. */
  kept: string[];
  /** Session id → the `updatedAt` it had when the owner last opened it. */
  seen: Record<string, number>;
  /** Notification ids marked read. */
  read: string[];
  /** Notification ids cleared with ✕. */
  cleared: string[];
}

const KEY = "sentinal.lifecycle";
const LIMIT = 400;
const EMPTY: LifecycleState = { autoClear: "1d", kept: [], seen: {}, read: [], cleared: [] };

function load(): LifecycleState {
  try {
    const raw = window.localStorage.getItem(KEY);
    if (!raw) return EMPTY;
    const parsed = JSON.parse(raw) as Partial<LifecycleState>;
    return {
      autoClear: parsed.autoClear && parsed.autoClear in AUTO_CLEAR_MS ? parsed.autoClear : EMPTY.autoClear,
      kept: Array.isArray(parsed.kept) ? parsed.kept : [],
      seen: parsed.seen && typeof parsed.seen === "object" ? parsed.seen : {},
      read: Array.isArray(parsed.read) ? parsed.read : [],
      cleared: Array.isArray(parsed.cleared) ? parsed.cleared : [],
    };
  } catch {
    return EMPTY;
  }
}

let state: LifecycleState | null = null;
const listeners = new Set<() => void>();

function current(): LifecycleState {
  if (!state) state = load();
  return state;
}

function update(change: (previous: LifecycleState) => LifecycleState) {
  const next = change(current());
  // Old ids drop off so the list stays small.
  state = { ...next, read: next.read.slice(-LIMIT), cleared: next.cleared.slice(-LIMIT) };
  try {
    window.localStorage.setItem(KEY, JSON.stringify(state));
  } catch {
    // Private windows: keep it for this page only.
  }
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useLifecycle(): LifecycleState {
  return useSyncExternalStore(subscribe, current, current);
}

/** For tests: forget everything. */
export function resetLifecycle() {
  state = null;
  try {
    window.localStorage.removeItem(KEY);
  } catch {
    // ignore
  }
  for (const listener of listeners) listener();
}

export const lifecycle = {
  get: current,
  setAutoClear: (autoClear: AutoClear) => update((previous) => ({ ...previous, autoClear })),
  setKept: (sessionId: string, kept: boolean) =>
    update((previous) => ({
      ...previous,
      kept: kept ? [...new Set([...previous.kept, sessionId])] : previous.kept.filter((id) => id !== sessionId),
    })),
  /** Opening a terminal marks it seen and reads its notifications. */
  markSeen: (session: Session) =>
    update((previous) => ({
      ...previous,
      seen: { ...previous.seen, [session.id]: session.updatedAt ?? 0 },
      read: [...new Set([...previous.read, notificationId(session)])],
    })),
  markRead: (ids: string[]) => update((previous) => ({ ...previous, read: [...new Set([...previous.read, ...ids])] })),
  clear: (ids: string[]) =>
    update((previous) => ({
      ...previous,
      read: [...new Set([...previous.read, ...ids])],
      cleared: [...new Set([...previous.cleared, ...ids])],
    })),
};

/** Finished well or was stopped: the Done list. FAILED stays under Needs you. */
export function isDone(session: Session): boolean {
  return session.state === "COMPLETED" || session.state === "INTERRUPTED";
}

export function isFinished(session: Session): boolean {
  return isDone(session) || session.state === "FAILED";
}

/** The owner has opened it since it last changed state. */
export function wasSeen(session: Session, seen: Record<string, number>): boolean {
  const at = seen[session.id];
  return at !== undefined && at >= (session.updatedAt ?? 0);
}

/** When auto-clear will hide it, or null when it never will. */
export function clearsAt(session: Session, life: LifecycleState): number | null {
  if (!isFinished(session) || session.dismissed || life.kept.includes(session.id)) return null;
  if (life.autoClear === "never" || session.updatedAt === undefined) return null;
  // A failure is never cleared before someone has looked at it.
  if (session.state === "FAILED" && !wasSeen(session, life.seen)) return null;
  return session.updatedAt + AUTO_CLEAR_MS[life.autoClear];
}

/** The finished Sessions auto-clear should hide now. Never deletes. */
export function autoClearDue(sessions: Session[], life: LifecycleState, now: number): string[] {
  return sessions.filter((session) => {
    const at = clearsAt(session, life);
    return at !== null && at <= now;
  }).map((session) => session.id);
}

/** "clears in 22h" / "clears in 40m". */
export function clearsInText(at: number, now: number): string {
  const minutes = Math.max(1, Math.round((at - now) / 60_000));
  return minutes >= 60 ? `clears in ${Math.round(minutes / 60)}h` : `clears in ${minutes}m`;
}

export type InboxGroup = "needs" | "done";

export interface InboxItem {
  /** One per Session state change, so a new failure is new again. */
  id: string;
  session: Session;
  group: InboxGroup;
  unread: boolean;
}

const INBOX_STATES: Partial<Record<Session["state"], InboxGroup>> = {
  WAITING: "needs",
  STALLED: "needs",
  FAILED: "needs",
  COMPLETED: "done",
  INTERRUPTED: "done",
};

export function notificationId(session: Session): string {
  return `${session.id}:${session.state}:${session.updatedAt ?? 0}`;
}

/** Notifications, needs-you first then newest first; cleared and hidden ones are gone. */
export function inboxItems(terminals: Session[], life: LifecycleState): InboxItem[] {
  const read = new Set(life.read);
  const cleared = new Set(life.cleared);
  return terminals
    .filter((session) => !session.dismissed && INBOX_STATES[session.state] && !cleared.has(notificationId(session)))
    .map((session) => ({
      id: notificationId(session),
      session,
      group: INBOX_STATES[session.state] as InboxGroup,
      unread: !read.has(notificationId(session)),
    }))
    .sort(
      (left, right) =>
        (left.group === "needs" ? 0 : 1) - (right.group === "needs" ? 0 : 1) ||
        (right.session.updatedAt ?? 0) - (left.session.updatedAt ?? 0),
    );
}
