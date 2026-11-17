import { afterEach, describe, expect, it } from "vitest";

import { autoClearDue, clearsAt, inboxItems, lifecycle, notificationId, resetLifecycle, type LifecycleState } from "./lifecycle";
import type { Session } from "./types";

const HOUR = 3_600_000;
const base: LifecycleState = { autoClear: "1d", kept: [], seen: {}, read: [], cleared: [] };

function session(id: string, state: Session["state"], updatedAt: number, extra: Partial<Session> = {}): Session {
  return { id, machine: "m", name: id, state, updatedAt, ...extra };
}

afterEach(() => resetLifecycle());

describe("auto-clear", () => {
  const now = 100 * HOUR;

  it("hides finished runs once they are older than the setting, and never live ones", () => {
    const sessions = [
      session("old-done", "COMPLETED", now - 25 * HOUR),
      session("new-done", "COMPLETED", now - 2 * HOUR),
      session("stopped", "INTERRUPTED", now - 30 * HOUR),
      session("live", "RUNNING", now - 50 * HOUR),
    ];
    expect(autoClearDue(sessions, base, now)).toEqual(["old-done", "stopped"]);
    expect(autoClearDue(sessions, { ...base, autoClear: "1h" }, now)).toEqual(["old-done", "new-done", "stopped"]);
    expect(autoClearDue(sessions, { ...base, autoClear: "never" }, now)).toEqual([]);
  });

  it("skips kept and already hidden Sessions", () => {
    const sessions = [session("kept", "COMPLETED", 0), session("hidden", "COMPLETED", 0, { dismissed: true })];
    expect(autoClearDue(sessions, { ...base, kept: ["kept"] }, now)).toEqual([]);
  });

  it("never clears a failure nobody has opened", () => {
    const failed = session("failed", "FAILED", 0);
    expect(clearsAt(failed, base)).toBeNull();
    expect(autoClearDue([failed], { ...base, seen: { failed: 0 } }, now)).toEqual(["failed"]);
  });
});

describe("inbox", () => {
  it("lists needs-you first, then done, newest first", () => {
    const items = inboxItems(
      [session("a", "COMPLETED", 5), session("b", "WAITING", 1), session("c", "FAILED", 3), session("d", "RUNNING", 9)],
      base,
    );
    expect(items.map((item) => item.session.id)).toEqual(["c", "b", "a"]);
    expect(items.every((item) => item.unread)).toBe(true);
  });

  it("opening a terminal reads its notification; a new state is new again", () => {
    const done = session("a", "COMPLETED", 5);
    lifecycle.markSeen(done);
    expect(inboxItems([done], lifecycle.get())[0].unread).toBe(false);
    const failedLater = session("a", "FAILED", 9);
    expect(inboxItems([failedLater], lifecycle.get())[0].unread).toBe(true);
  });

  it("✕ clears the notification but not the terminal", () => {
    const done = session("a", "COMPLETED", 5);
    lifecycle.clear([notificationId(done)]);
    expect(inboxItems([done], lifecycle.get())).toEqual([]);
  });
});
