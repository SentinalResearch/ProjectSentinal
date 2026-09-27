import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { KeyBar } from "./keybar";

describe("KeyBar", () => {
  it.each([
    ["Ctrl-C", [3]],
    ["Esc", [27]],
    ["Tab", [9]],
    ["↑", [27, 91, 65]],
    ["↓", [27, 91, 66]],
    ["y", [121]],
    ["n", [110]],
  ])("sends the bytes for %s", async (label, expected) => {
    const onBytes = vi.fn();
    render(<KeyBar onBytes={onBytes} />);

    await userEvent.click(screen.getByRole("button", { name: label }));

    expect(onBytes).toHaveBeenCalledWith(new Uint8Array(expected as number[]));
  });
});
