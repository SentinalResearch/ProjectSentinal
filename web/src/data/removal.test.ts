import { describe, expect, it } from "vitest";

import { friendlyRemovalError, removalSupport } from "./removal";

const machine = (daemonVersion?: string) => ({ name: "sentinal-research", daemonVersion });

describe("removalSupport", () => {
  it("lets a 2.11 Machine hide, restore and delete any finished Session", () => {
    expect(removalSupport({ state: "INTERRUPTED" }, machine("2.11.0"))).toEqual({
      hide: true,
      restore: true,
      remove: true,
    });
  });

  it("lets a 2.10 Machine hide only a failed Session, and says what it needs", () => {
    expect(removalSupport({ state: "FAILED" }, machine("2.10.0"))).toMatchObject({ hide: true, remove: false });
    expect(removalSupport({ state: "INTERRUPTED" }, machine("2.10.0"))).toEqual({
      hide: false,
      restore: false,
      remove: false,
      note: "sentinal-research runs Sentinal 2.10.0. Hiding a stopped Session needs 2.11.",
    });
  });

  it("knows an older 0.2.x Machine cannot hide anything", () => {
    expect(removalSupport({ state: "FAILED" }, machine("0.2.4"))).toEqual({
      hide: false,
      restore: false,
      remove: false,
      note: "sentinal-research runs Sentinal 0.2.4. Hiding and deleting Sessions need 2.11.",
    });
  });

  it("does not block a Machine whose version is unknown", () => {
    expect(removalSupport({ state: "COMPLETED" }, machine(undefined))).toEqual({
      hide: true,
      restore: true,
      remove: true,
    });
    expect(removalSupport({ state: "COMPLETED" }, undefined).hide).toBe(true);
  });

  it("treats a development build (0.0.x, the package version) as current", () => {
    expect(removalSupport({ state: "INTERRUPTED" }, machine("0.0.1"))).toEqual({
      hide: true,
      restore: true,
      remove: true,
    });
  });
});

describe("friendlyRemovalError", () => {
  it("never shows a raw daemon decoding error", () => {
    const raw =
      "daemon request failed: invalid JSON: unknown variant `DISMISS_SESSION`, expected one of `GET_MACHINE`, `LIST_SESSIONS` at line 1 column 48";
    expect(friendlyRemovalError(raw)).toBe(
      "This Machine runs an older Sentinal that cannot hide or delete Sessions. Update it to 2.11.",
    );
  });

  it("explains the 2.10 rule", () => {
    expect(
      friendlyRemovalError("daemon request failed: Session #4 is Interrupted; only a failed Session can be dismissed"),
    ).toBe("This Machine runs Sentinal 2.10, which hides only failed Sessions. Update it to 2.11 to hide this one.");
  });

  it("keeps other messages, without the transport prefix", () => {
    expect(friendlyRemovalError("daemon request failed: Session 9 is Running; stop it first")).toBe(
      "Session 9 is Running; stop it first",
    );
  });
});
