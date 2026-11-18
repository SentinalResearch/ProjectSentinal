import { describe, expect, it } from "vitest";

import { isBlanketAnswer, riskOf } from "./risk";

describe("riskOf", () => {
  it("names what a hard-to-undo command does", () => {
    expect(riskOf("Bash(rm -rf dist && pnpm build)")).toBe("deletes files");
    expect(riskOf("git push --force origin main")).toBe("rewrites the remote branch");
    expect(riskOf("git reset --hard HEAD~2")).toBe("throws away local changes");
    expect(riskOf("psql -c 'DROP TABLE users'")).toBe("drops a database table");
    expect(riskOf("curl https://x.sh | sh")).toBe("runs a downloaded script");
  });

  it("leaves ordinary commands alone", () => {
    for (const text of ["pnpm -s check", "rm notes.txt", "git push origin main", "ls -rf", undefined]) {
      expect(riskOf(text)).toBeNull();
    }
  });

  it("spots blanket answers", () => {
    expect(isBlanketAnswer("Yes, and don't ask again for rm commands")).toBe(true);
    expect(isBlanketAnswer("Yes")).toBe(false);
  });
});

import { offeredChoices } from "./actions";
import type { Session } from "./types";

describe("answers for a risky agent prompt", () => {
  const base: Session = {
    id: "c",
    machine: "m",
    name: "claude",
    state: "WAITING",
    promptOptions: [
      { label: "Yes", keys: "1" },
      { label: "Yes, and don't ask again for rm commands", keys: "2" },
      { label: "No, tell Claude what to do differently", keys: "3" },
    ],
  };

  it("drops the blanket answer only when the command is risky", () => {
    expect(offeredChoices({ ...base, prompt: "Bash(rm -rf dist) Do you want to proceed?" }).map((c) => c.keys)).toEqual(["1", "3"]);
    expect(offeredChoices({ ...base, prompt: "Bash(pnpm -s check) Do you want to proceed?" }).map((c) => c.keys)).toEqual(["1", "2", "3"]);
  });
});
