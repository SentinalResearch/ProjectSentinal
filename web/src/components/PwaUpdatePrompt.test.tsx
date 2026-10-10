import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { PwaUpdatePrompt } from "./PwaUpdatePrompt";

describe("PwaUpdatePrompt", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("offers to activate a service worker that was already waiting before the page loaded", async () => {
    const postMessage = vi.fn();
    const registration = {
      waiting: { postMessage },
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    };
    Object.defineProperty(navigator, "serviceWorker", {
      configurable: true,
      value: {
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        getRegistration: vi.fn().mockResolvedValue(registration),
      },
    });

    render(<PwaUpdatePrompt />);

    await userEvent.setup().click(await screen.findByRole("button", { name: "Refresh to update" }));
    expect(postMessage).toHaveBeenCalledWith({ type: "SKIP_WAITING" });
  });
});
