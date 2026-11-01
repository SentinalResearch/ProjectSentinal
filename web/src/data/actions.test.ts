import { afterEach, describe, expect, it, vi } from "vitest";

import { answerPrompt, canAnswer, canInterrupt } from "./actions";
import { dataAdapter } from "./index";
import type { Session } from "./types";

afterEach(() => vi.restoreAllMocks());

const waiting: Session = { id: "m:1", machine: "m", name: "npm install", state: "WAITING", prompt: "Continue? [y/N]" };

describe("answering a prompt", () => {
  it("sends the key and Enter", async () => {
    // Desktop used to send a bare "y", which a line-reading [y/N] prompt ignores.
    const send = vi.spyOn(dataAdapter, "sendInput").mockResolvedValue();
    await answerPrompt("m:1", "y");
    expect(send).toHaveBeenCalledWith("m:1", "y\r");
  });

  it("is offered only for a waiting prompt the owner may type into", () => {
    expect(canAnswer(waiting)).toBe(true);
    expect(canAnswer({ ...waiting, prompt: undefined })).toBe(false);
    expect(canAnswer({ ...waiting, capture: "PROCESS_LOG" })).toBe(false);
    expect(canAnswer({ ...waiting, state: "RUNNING" })).toBe(false);
  });
});

describe("interrupting", () => {
  it("needs a live Session that accepts input", () => {
    expect(canInterrupt({ ...waiting, state: "RUNNING" })).toBe(true);
    expect(canInterrupt({ ...waiting, state: "COMPLETED" })).toBe(false);
    expect(canInterrupt({ ...waiting, state: "RUNNING", capture: "COMMANDS" })).toBe(false);
  });
});
