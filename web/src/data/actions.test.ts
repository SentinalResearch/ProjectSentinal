import { afterEach, describe, expect, it, vi } from "vitest";

import { answerChoice, answerPrompt, canAnswer, canInterrupt, noAnswerReason } from "./actions";
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

describe("noAnswerReason", () => {
  it("says why a watched-only Session cannot be answered here", () => {
    expect(noAnswerReason({ ...waiting, capture: "PROCESS_LOG" })).toBe(
      "Sentinal only watches this one. Answer it in its own terminal.",
    );
  });

  it("is null when the card can answer, or nothing is waiting", () => {
    expect(noAnswerReason(waiting)).toBeNull();
    expect(noAnswerReason({ ...waiting, state: "RUNNING" })).toBeNull();
  });

  it("points free-text prompts to the terminal", () => {
    expect(noAnswerReason({ ...waiting, prompt: "Password:" })).toBe("Answer it in the terminal.");
  });
});

describe("answering a choice (P1)", () => {
  it("sends the option's own key for a non-Vibe agent", async () => {
    const send = vi.spyOn(dataAdapter, "sendInput").mockResolvedValue();
    const session: Session = {
      id: "m:2",
      machine: "m",
      name: "claude",
      state: "WAITING",
      prompt: "Proceed?",
      codingAgent: { agent: "CLAUDE", working: true, toolUses: 1 },
    };
    await answerChoice("m:2", { label: "No", keys: "3" }, session);
    expect(send).toHaveBeenCalledWith("m:2", "3");
  });

  it("offers no one-tap answer for Vibe, whose marker position is unknown", async () => {
    // Down x (N-1) assumed the marker starts on option 1; if it does not, the
    // wrong answer reaches a permission prompt. Until the watcher reports where
    // the marker is, a Vibe question is answered in the terminal.
    const send = vi.spyOn(dataAdapter, "sendInput").mockResolvedValue();
    const vibe: Session = {
      id: "m:3",
      machine: "m",
      name: "vibe",
      state: "WAITING",
      prompt: "Allow?",
      promptOptions: [
        { label: "Yes", keys: "1" },
        { label: "No", keys: "2" },
      ],
      codingAgent: { agent: "VIBE", working: true, toolUses: 0 },
    };
    expect(canAnswer(vibe)).toBe(false);
    await expect(answerChoice("m:3", { label: "No", keys: "2" }, vibe)).rejects.toThrow();
    expect(send).not.toHaveBeenCalled();
  });

  it("refuses keys that are not an allowed choice", async () => {
    vi.spyOn(dataAdapter, "sendInput").mockResolvedValue();
    await expect(answerChoice("m:1", { label: "evil", keys: "rm -rf" }, waiting)).rejects.toThrow("not an answer key");
  });
});

describe("interrupting", () => {
  it("needs a live Session that accepts input", () => {
    expect(canInterrupt({ ...waiting, state: "RUNNING" })).toBe(true);
    expect(canInterrupt({ ...waiting, state: "COMPLETED" })).toBe(false);
    expect(canInterrupt({ ...waiting, state: "RUNNING", capture: "COMMANDS" })).toBe(false);
  });
});

describe("what an answer button may send", () => {
  const waiting = (prompt: string) =>
    ({ id: "m:1", state: "WAITING", prompt, capture: "FULL" }) as unknown as Parameters<typeof canAnswer>[0];

  it("offers y and n only for a yes-or-no question", () => {
    for (const prompt of ["Publish the results? [y/N]", "Continue [Y/n]", "Overwrite (y/n)?", "Proceed? [yes/no]"]) {
      expect(canAnswer(waiting(prompt)), prompt).toBe(true);
    }
  });

  it("never offers y or n to a question those keys cannot answer", () => {
    for (const prompt of ["Password:", "Press any key", "Are you sure? (yes/no)", "Which file? "]) {
      expect(canAnswer(waiting(prompt)), prompt).toBe(false);
    }
  });

  it("offers a program's own listed answers whatever the wording", () => {
    const session = { ...waiting("Allow this?"), promptOptions: [{ label: "Allow", keys: "1" }] };
    expect(canAnswer(session)).toBe(true);
  });
});
