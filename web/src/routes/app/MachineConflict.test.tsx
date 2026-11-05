import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it } from "vitest";

import { LiveDataProvider } from "../../data/LiveDataContext";
import { machineSummary } from "../../data/format";
import type { Machine } from "../../data/types";
import { MachinePanel } from "./MachinePanel";

const machine = {
  id: "m",
  name: "Mac Mini",
  os: "macOS",
  hardware: "",
  status: "ONLINE",
  daemonVersion: "0.2.9",
  buildCommit: "27357f9",
} as Machine;

describe("which install speaks for a Machine", () => {
  it("names the version and its build hash", () => {
    expect(machineSummary(machine)).toBe("Sentinal 0.2.9 (27357f9)");
    expect(machineSummary({ ...machine, buildCommit: undefined })).toBe("Sentinal 0.2.9");
  });

  it("tells the owner when another install tried to use this Machine", async () => {
    render(
      <MemoryRouter>
        <LiveDataProvider>
          <MachinePanel
            machine={{ ...machine, instanceConflict: { daemonVersion: "2.10.0", buildCommit: "6b7936e-dirty", port: 7520 } }}
            sessions={[]}
          />
        </LiveDataProvider>
      </MemoryRouter>,
    );
    const alerts = await screen.findAllByRole("alert");
    const alert = alerts.find((entry) => entry.textContent?.includes("Another Sentinal install"));
    if (!alert) throw new Error("no conflict warning");
    expect(alert).toHaveTextContent("port 7520");
    expect(alert).toHaveTextContent("6b7936e-dirty");
  });
});
