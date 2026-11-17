import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { lifecycle, resetLifecycle } from "../../../data/lifecycle";
import type { Session } from "../../../data/types";
import { DoneSection } from "./DoneSection";

const now = 10 * 3_600_000;
const sessions: Session[] = [
  { id: "a", machine: "m", name: "train.py", state: "COMPLETED", updatedAt: now - 3_600_000 },
  { id: "b", machine: "m", name: "pytest", state: "FAILED", updatedAt: now },
  { id: "c", machine: "m", name: "vite", state: "INTERRUPTED", updatedAt: now, dismissed: true },
];

afterEach(() => resetLifecycle());

describe("DoneSection", () => {
  it("lists finished terminals only, with when they clear", () => {
    render(<DoneSection sessions={sessions} machineName={() => "RTX-PC"} now={now} onOpen={() => undefined} />);
    expect(screen.getByRole("heading", { name: /Done/ })).toHaveTextContent("1");
    expect(screen.getByText("train.py")).toBeInTheDocument();
    expect(screen.queryByText("pytest")).toBeNull();
    expect(screen.getByText(/clears in 23h/)).toBeInTheDocument();
  });

  it("asks before deleting and cycles auto-clear", () => {
    render(<DoneSection sessions={sessions} machineName={() => "RTX-PC"} now={now} onOpen={() => undefined} />);
    fireEvent.click(screen.getByRole("button", { name: "Delete train.py" }));
    expect(screen.getByText(/Permanently delete train.py/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Auto-clear: after 1 day/ }));
    expect(lifecycle.get().autoClear).toBe("never");
  });

  it("shows nothing when nothing is done", () => {
    const { container } = render(<DoneSection sessions={[sessions[1]]} machineName={() => ""} now={now} onOpen={() => undefined} />);
    expect(container).toBeEmptyDOMElement();
  });
});
