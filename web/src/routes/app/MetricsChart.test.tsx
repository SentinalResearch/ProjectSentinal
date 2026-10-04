import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { MetricsChart } from "./MetricsChart";

describe("MetricsChart", () => {
  it("prints now, min, average, and max for each resource beside the graph", () => {
    render(
      <MetricsChart
        range="HOUR"
        samples={[
          { at: 0, cpuPct: 10, ramPct: 40 },
          { at: 15_000, cpuPct: 50, ramPct: 50, gpuPct: 90 },
          { at: 30_000, cpuPct: 30, ramPct: 60, gpuPct: 70 },
        ]}
      />,
    );
    const table = screen.getByRole("table", { name: "Machine load summary" });
    const cpu = within(table).getByRole("row", { name: /CPU/ });
    expect(within(cpu).getAllByRole("cell").map((cell) => cell.textContent)).toEqual(["30 %", "10 %", "30 %", "50 %"]);
    const gpu = within(table).getByRole("row", { name: /GPU/ });
    expect(within(gpu).getAllByRole("cell").map((cell) => cell.textContent)).toEqual(["70 %", "70 %", "80 %", "90 %"]);
    expect(within(table).queryByRole("row", { name: /VRAM/ })).not.toBeInTheDocument();
  });

  it("says when there is no history yet", () => {
    render(<MetricsChart range="DAY" samples={[]} />);
    expect(screen.getByText("No samples for the last 24 hours yet.")).toBeInTheDocument();
  });
});
