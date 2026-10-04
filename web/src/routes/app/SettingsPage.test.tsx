import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeAll, beforeEach, describe, expect, it } from "vitest";

import { AuthProvider } from "../../context/AuthContext";
import { ThemeProvider } from "../../context/ThemeContext";
import { AppShell } from "./AppShell";
import { SettingsPage } from "./SettingsPage";
import styles from "./SettingsPage.module.css";
import { resetSimulation } from "../../data/mock/simulation";

function renderSettings(path: string) {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <ThemeProvider>
        <AuthProvider>
          <Routes>
            <Route path="/app" element={<AppShell />}>
              <Route path="settings" element={<SettingsPage />} />
              <Route path="settings/:tab" element={<SettingsPage />} />
            </Route>
          </Routes>
        </AuthProvider>
      </ThemeProvider>
    </MemoryRouter>,
  );
}

describe("SettingsPage", () => {
  beforeAll(() => {
    Object.defineProperty(window, "innerWidth", { writable: true, configurable: true, value: 1440 });
  });

  beforeEach(() => resetSimulation());

  it("defaults to Machines and switches tabs", async () => {
    renderSettings("/app/settings");

    expect(await screen.findByRole("heading", { name: "Machines" }, { timeout: 5000 })).toBeInTheDocument();

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Notifications" }));
    expect(await screen.findByRole("heading", { name: "Notifications" }, { timeout: 5000 })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Account" }));
    expect(await screen.findByRole("heading", { name: "Account" }, { timeout: 5000 })).toBeInTheDocument();
  });

  it("revoking a Machine names it in the inline confirmation", async () => {
    renderSettings("/app/settings/machines");
    const user = userEvent.setup();

    // "RTX-PC" also appears in the sidebar's MACHINES section — wait for the settings
    // table's own row (a separate async registry.list() fetch from the sidebar's
    // own data) before scoping to it, so this doesn't race the sidebar's render.
    // Generous timeout: under the full suite's parallel xterm/WebGL-heavy test files,
    // this environment can be slow enough to exceed the 1000ms findBy default.
    const row = await waitFor(
      () => {
        const nameSpan = screen.getAllByText("RTX-PC").find((el) => el.closest(`.${styles.machineRow}`));
        const found = nameSpan?.closest(`.${styles.machineRow}`) as HTMLElement | null;
        if (!found) throw new Error("RTX-PC row not found yet");
        return found;
      },
      { timeout: 5000 },
    );

    await user.click(within(row).getByRole("button", { name: "Revoke…" }));
    expect(within(row).getByText("Revoke RTX-PC?")).toBeInTheDocument();

    // Keep cancels without revoking.
    await user.click(within(row).getByRole("button", { name: "Keep" }));
    expect(within(row).queryByText("Revoke RTX-PC?")).not.toBeInTheDocument();
    expect(within(row).getByRole("button", { name: "Revoke…" })).toBeInTheDocument();

    // Confirming revokes the Machine; its row stays, marked Revoked, for history.
    await user.click(within(row).getByRole("button", { name: "Revoke…" }));
    await user.click(within(row).getByRole("button", { name: "Revoke" }));
    await waitFor(
      () => {
        const revoked = screen.getAllByText("RTX-PC").find((el) => el.closest(`.${styles.machineRow}`));
        expect(within(revoked?.closest(`.${styles.machineRow}`) as HTMLElement).getByText("Revoked")).toBeInTheDocument();
      },
      { timeout: 5000 },
    );
  });

  it("Add Machine shows a one-time code and a command without it", async () => {
    renderSettings("/app/settings/machines");
    const user = userEvent.setup();

    await user.click(await screen.findByRole("button", { name: "Add Machine" }, { timeout: 5000 }));

    const dialog = await screen.findByRole("dialog", { name: "Add Machine" }, { timeout: 5000 });
    const code = await within(dialog).findByText(/^[0-9A-Z]{5}-[0-9A-Z]{5}$/, {}, { timeout: 5000 });
    const command = within(dialog).getByText(/^sentinal connect https:/);
    expect(command.textContent).not.toContain(code.textContent ?? "");
    expect(within(dialog).getByText(/Expires in/)).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("offers repeatable mock-only simulation controls", async () => {
    renderSettings("/app/settings/simulation");

    expect(await screen.findByRole("heading", { name: "User testing simulation" })).toBeInTheDocument();
    expect(screen.getByText("Fixtures only — no daemon is connected")).toBeInTheDocument();

    const user = userEvent.setup();
    await user.selectOptions(screen.getByLabelText("Scenario"), "needs-attention");
    await user.click(screen.getByRole("button", { name: "Apply scenario" }));

    expect(await screen.findByText("4 Sessions · 2 Machines")).toBeInTheDocument();
    expect(screen.getByText("4 need attention")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Reset mixed demo" }));
    expect(await screen.findByText("10 Sessions · 3 Machines")).toBeInTheDocument();
  });
});
