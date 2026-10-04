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

  it("fills the RAM bar against the Machine's total RAM", () => {
    const { container } = render(<ResourceBars metrics={{ ramGB: 4, ramTotalGB: 16 }} />);
    expect(screen.getByText("4 / 16 GB")).toBeInTheDocument();
    expect(container.querySelector<HTMLElement>("[style*='width']")?.style.width).toBe("25%");
  });

  it("renders nothing without available metrics", () => {
    const { container } = render(<ResourceBars metrics={undefined} />);
    expect(container).toBeEmptyDOMElement();
  });
});
