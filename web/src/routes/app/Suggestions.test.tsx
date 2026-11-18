import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";

import { dataAdapter } from "../../data";
import type { Session } from "../../data/types";
import { Suggestions } from "./Suggestions";

afterEach(() => vi.restoreAllMocks());

const session: Session = {
  id: "pytest",
  machine: "rtx",
  name: "pytest",
  state: "FAILED",
  cwd: "~/work",
  suggestions: [{ command: "pytest tests/test_match.py -x", why: "re-runs only the failing tests", inferred: true }],
};

describe("Suggestions", () => {
  it("runs nothing until tapped, then starts a terminal on the same Machine and types the command", async () => {
    const create = vi.spyOn(dataAdapter, "createSession").mockResolvedValue({ ...session, id: "new", state: "CREATED" });
    const send = vi.spyOn(dataAdapter, "sendInput").mockResolvedValue();
    render(
      <MemoryRouter>
        <Suggestions session={session} />
      </MemoryRouter>,
    );
    expect(create).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Run pytest tests/test_match.py -x in a new terminal" }));
    await waitFor(() => expect(send).toHaveBeenCalledWith("new", "pytest tests/test_match.py -x\r"));
    expect(create).toHaveBeenCalledWith(expect.objectContaining({ machineId: "rtx", cwd: "~/work" }));
  });

  it("shows nothing without suggestions", () => {
    const { container } = render(
      <MemoryRouter>
        <Suggestions session={{ ...session, suggestions: undefined }} />
      </MemoryRouter>,
    );
    expect(container).toBeEmptyDOMElement();
  });
});
