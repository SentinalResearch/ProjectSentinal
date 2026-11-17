import { fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it } from "vitest";

import { resetLifecycle } from "../../../data/lifecycle";
import type { Session } from "../../../data/types";
import { IslandInbox } from "./IslandInbox";

const terminals: Session[] = [
  { id: "a", machine: "m", name: "train.py", state: "COMPLETED", updatedAt: 5 },
  { id: "b", machine: "m", name: "npm", state: "WAITING", prompt: "Continue? [y/N]", updatedAt: 3 },
  { id: "c", machine: "m", name: "vllm", state: "RUNNING", updatedAt: 9 },
];

afterEach(() => resetLifecycle());

function renderInbox() {
  return render(
    <MemoryRouter>
      <IslandInbox terminals={terminals} machineName={() => "Mac Mini"} onClose={() => undefined} />
    </MemoryRouter>,
  );
}

describe("IslandInbox", () => {
  it("groups needs-you before done and counts what is new", () => {
    renderInbox();
    expect(screen.getByText("2 new")).toBeInTheDocument();
    const groups = screen.getAllByRole("region").map((region) => region.getAttribute("aria-label"));
    expect(groups).toEqual(["Needs you", "Done"]);
    expect(screen.queryByText(/vllm/)).toBeNull();
  });

  it("✕ clears a notification; Mark all read reads the rest", () => {
    renderInbox();
    fireEvent.click(screen.getByRole("button", { name: "Clear the notification for train.py" }));
    expect(screen.queryByText(/train.py/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Mark all read" }));
    expect(screen.queryByText(/new/)).toBeNull();
  });
});
