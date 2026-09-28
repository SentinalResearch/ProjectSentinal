import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { ResourceBars } from "./ResourceBars";

describe("ResourceBars", () => {
  it("shows live CPU RAM GPU and VRAM values", () => {
    render(
      <ResourceBars
        metrics={{ cpuPct: 87.5, ramGB: 3.2, gpuPct: 96, vramGB: 21.4, vramTotalGB: 24 }}
      />,
    );

    expect(screen.getByText("87.5 %")).toBeInTheDocument();
    expect(screen.getByText("3.2 GB")).toBeInTheDocument();
    expect(screen.getByText("96 %")).toBeInTheDocument();
    expect(screen.getByText("21.4 / 24 GB")).toBeInTheDocument();
  });

  it("renders nothing without available metrics", () => {
    const { container } = render(<ResourceBars metrics={undefined} />);
    expect(container).toBeEmptyDOMElement();
  });
});
