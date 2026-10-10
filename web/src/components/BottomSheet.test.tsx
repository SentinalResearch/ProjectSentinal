import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";

import { BottomSheet } from "./BottomSheet";

function Harness({ onConfirm }: { onConfirm: () => void }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button type="button" onClick={() => setOpen(true)}>Stop</button>
      <BottomSheet open={open} title="Send Ctrl-C to Qwen Training?" onClose={() => setOpen(false)}>
        <p>This interrupts python train.py.</p>
        <button type="button" onClick={onConfirm}>Send Ctrl-C</button>
        <button type="button" onClick={() => setOpen(false)}>Cancel</button>
      </BottomSheet>
    </>
  );
}

describe("BottomSheet", () => {
  it("is a named modal dialog that starts on Cancel and traps focus", async () => {
    render(<Harness onConfirm={() => undefined} />);
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Stop" }));
    const dialog = screen.getByRole("dialog", { name: "Send Ctrl-C to Qwen Training?" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
    await user.tab();
    expect(dialog.contains(document.activeElement)).toBe(true);
    await user.tab();
    await user.tab();
    expect(dialog.contains(document.activeElement)).toBe(true);
  });

  it("closes on Escape and on the scrim without confirming, and returns focus", async () => {
    const onConfirm = vi.fn();
    render(<Harness onConfirm={onConfirm} />);
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Stop" }));
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Stop" })).toHaveFocus();
    await user.click(screen.getByRole("button", { name: "Stop" }));
    await user.click(screen.getByTestId("sheet-scrim"));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(onConfirm).not.toHaveBeenCalled();
  });
});
