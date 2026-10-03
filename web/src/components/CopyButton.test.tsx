import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { CopyButton } from "./CopyButton";

function clipboard(writeText: (text: string) => Promise<void>) {
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
}

afterEach(() => vi.useRealTimers());

describe("CopyButton", () => {
  it("copies, confirms visibly and to screen readers, then reverts", async () => {
    vi.useFakeTimers();
    const writeText = vi.fn().mockResolvedValue(undefined);
    clipboard(writeText);
    render(<CopyButton text="7KQ4M-2XZ9C" label="Copy code" resetMs={2000} />);

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Copy code" }));
    });

    expect(writeText).toHaveBeenCalledWith("7KQ4M-2XZ9C");
    expect(screen.getByRole("button", { name: "Copied to clipboard" })).toHaveTextContent("Copied to clipboard");
    expect(screen.getByRole("status")).toHaveTextContent("Copied to clipboard");
    act(() => vi.advanceTimersByTime(2000));
    expect(screen.getByRole("button", { name: "Copy code" })).toHaveTextContent("Copy code");
    expect(screen.getByRole("status")).toHaveTextContent("");
  });

  it("says so when the clipboard refuses instead of claiming success", async () => {
    clipboard(vi.fn().mockRejectedValue(new Error("denied")));
    render(<CopyButton text="x" label="Copy" />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Copy" }));
    });
    expect(screen.getByRole("button")).toHaveTextContent("Copy failed — select and copy");
    expect(screen.getByRole("status")).toHaveTextContent("");
  });
});
