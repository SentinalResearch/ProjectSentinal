import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { dataAdapter } from "../../data";
import { AddMachineDialog } from "./AddMachineDialog";

afterEach(() => vi.restoreAllMocks());

describe("Add Machine dialog", () => {
  it("still sees the worker connect when its parent re-renders faster than it polls", async () => {
    // Under the glass shell, live data re-rendered Settings every few ms; a new
    // onConnected each time restarted the 2 s poll, which then never fired.
    vi.spyOn(dataAdapter.registry, "createCode").mockResolvedValue({
      code_id: "c1",
      code: "ABCDE-FGHIJ",
      expires_at: Date.now() + 600_000,
    } as never);
    vi.spyOn(dataAdapter.registry, "codeStatus").mockResolvedValue({ state: "CONNECTED", machine_id: "w1" } as never);
    vi.spyOn(dataAdapter.registry, "cancelCode").mockResolvedValue();
    const connected = vi.fn();
    const view = render(<AddMachineDialog onClose={() => {}} onConnected={() => connected()} pollMs={60} />);
    const churn = setInterval(() => {
      view.rerender(<AddMachineDialog onClose={() => {}} onConnected={() => connected()} pollMs={60} />);
    }, 10);
    try {
      await waitFor(() => expect(connected).toHaveBeenCalled(), { timeout: 2_000 });
      expect(screen.getByText("Connected", { exact: true })).toBeInTheDocument();
    } finally {
      clearInterval(churn);
    }
  });
});
