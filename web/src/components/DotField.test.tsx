import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { DotField } from "./DotField";

describe("DotField", () => {
  it("is decoration only and follows the screen's signal", () => {
    const { container, rerender } = render(<DotField signal="idle" />);
    const canvas = container.querySelector("canvas");
    expect(canvas).toHaveAttribute("aria-hidden", "true");
    expect(canvas).toHaveAttribute("data-signal", "idle");
    rerender(<DotField signal="busy" />);
    expect(canvas).toHaveAttribute("data-signal", "busy");
  });
});
