import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { EventsList } from "./EventsList";

describe("EventsList", () => {
  it("shows no detail that only repeats the label (QA 2026-11-19)", () => {
    render(
      <EventsList
        events={[
          ["11:20:10", "COMMAND_STARTED", "Command started", 1],
          ["11:20:15", "WAITING", "Waiting for input", 2],
          ["11:20:44", "COMMAND_FINISHED", "exit 0 · 34s", 3],
        ]}
      />,
    );
    expect(screen.getAllByText("Command started")).toHaveLength(1);
    expect(screen.getAllByText("Waiting for input")).toHaveLength(1);
    expect(screen.getByText("exit 0 · 34s")).toBeTruthy();
  });
});
