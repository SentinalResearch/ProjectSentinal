import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { EventsList } from "./EventsList";
import { SummaryLine } from "./SummaryLine";

describe("intelligence guesses", () => {
  it("labels a guessed summary, and not a known one", () => {
    const { rerender } = render(<SummaryLine summary={{ text: "Compressing shard 14", inferred: true }} />);
    expect(screen.getByText(/Sentinal thinks/)).toBeInTheDocument();
    rerender(<SummaryLine summary={{ text: "Compressing shard 14", inferred: false }} />);
    expect(screen.queryByText(/Sentinal thinks/)).toBeNull();
  });

  it("marks only inferred events and explains the mark", () => {
    render(
      <EventsList
        events={[
          ["13:20", "COMMAND_STARTED", "python compress.py"],
          ["13:52", "PROGRESS", "shard 14/40", undefined, true],
        ]}
      />,
    );
    expect(screen.getAllByText("Sentinal thinks")).toHaveLength(1);
    expect(screen.getByText(/Guesses never alert or act/)).toBeInTheDocument();
  });
});
