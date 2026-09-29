import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

afterEach(() => {
  cleanup();
  vi.resetModules();
  vi.clearAllMocks();
});

describe("MockBadge", () => {
  it("is absent while the UI uses the real daemon", async () => {
    vi.doMock("../data/mode", () => ({ isMockData: false }));
    const { MockBadge } = await import("./MockBadge");

    render(<MockBadge />);

    expect(screen.queryByText("MOCK")).not.toBeInTheDocument();
  });

  it("labels the explicit fixture demo", async () => {
    vi.doMock("../data/mode", () => ({ isMockData: true }));
    const { MockBadge } = await import("./MockBadge");

    render(<MockBadge />);

    expect(screen.getByText("MOCK")).toBeInTheDocument();
  });
});
