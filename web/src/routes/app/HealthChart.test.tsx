import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { HealthChart } from "./HealthChart";

describe("HealthChart", () => {
  it("draws a marker for every sample, so a single-sample series is visible instead of an empty box", () => {
    const { container } = render(
      <HealthChart metric="TYPING_ECHO_MS" title="Typing delay on mini" points={[{ at: 1000, p50: 8, p95: 69, count: 3 }]} />,
    );
    const svg = container.querySelector("svg");
    expect(svg).not.toBeNull();
    // A one-point polyline renders nothing; the marker circle is what carries the sample.
    expect(svg!.querySelectorAll("circle").length).toBe(1);
    expect(screen.queryByText("No samples in this range yet.")).not.toBeInTheDocument();
  });

  it("draws one marker per sample across a range", () => {
    const { container } = render(
      <HealthChart
        metric="DAEMON_RSS_BYTES"
        title="sentinald memory on mini"
        points={[
          { at: 0, p50: 20, p95: 24, count: 1440 },
          { at: 60_000, p50: 21, p95: 25, count: 1440 },
          { at: 120_000, p50: 24, p95: 25, count: 1440 },
        ]}
      />,
    );
    expect(container.querySelectorAll("svg circle").length).toBe(3);
  });

  it("says when there are no samples", () => {
    render(<HealthChart metric="LINK_RTT_MS" title="Link round trip on mini" points={[]} />);
    expect(screen.getByText("No samples in this range yet.")).toBeInTheDocument();
  });
});
