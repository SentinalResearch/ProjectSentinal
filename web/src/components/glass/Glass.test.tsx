import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { AttentionCard, GlassPanel, GlassPill } from "./Glass";

describe("glass primitives", () => {
  it("render the requested element with their recipe class and extra props", () => {
    render(
      <>
        <GlassPill as="nav" aria-label="island" className="extra" />
        <GlassPanel aria-label="lane" tile />
      </>,
    );
    const island = screen.getByRole("navigation", { name: "island" });
    expect(island.className).toMatch(/pill/);
    expect(island.className).toMatch(/extra/);
    expect(screen.getByRole("region", { name: "lane" }).className).toMatch(/tile/);
  });

  it("tint an attention card by its Session state", () => {
    render(<AttentionCard state="WAITING" aria-label="npm install" />);
    expect(screen.getByRole("article", { name: "npm install" }).dataset.state).toBe("WAITING");
  });
});
