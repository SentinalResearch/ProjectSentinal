import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeAll, describe, expect, it } from "vitest";

import { AuthProvider } from "../../context/AuthContext";
import { ThemeProvider } from "../../context/ThemeContext";
import { AppShell } from "./AppShell";
import { SettingsPage } from "./SettingsPage";
import styles from "./SettingsPage.module.css";

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

  it("defaults to Machines & tokens and switches tabs", async () => {
    renderSettings("/app/settings");

    expect(await screen.findByRole("heading", { name: "Machines & tokens" }, { timeout: 5000 })).toBeInTheDocument();

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Notifications" }));
    expect(await screen.findByRole("heading", { name: "Notifications" }, { timeout: 5000 })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Account" }));
    expect(await screen.findByRole("heading", { name: "Account" }, { timeout: 5000 })).toBeInTheDocument();
  });

  it("revoking a token requires the inline Revoke?/Revoke/Keep confirm step", async () => {
    renderSettings("/app/settings/machines");
    const user = userEvent.setup();

    // "RTX-PC" also appears in the sidebar's MACHINES section — wait for the settings
    // table's own row (a separate async getMachineTokens() fetch from the sidebar's
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
    const table = row.parentElement as HTMLElement;

    await user.click(within(row).getByRole("button", { name: "Revoke…" }));
    expect(within(row).getByText("Revoke?")).toBeInTheDocument();

    // Keep cancels without revoking.
    await user.click(within(row).getByRole("button", { name: "Keep" }));
    expect(within(row).queryByText("Revoke?")).not.toBeInTheDocument();
    expect(within(row).getByRole("button", { name: "Revoke…" })).toBeInTheDocument();

    // Confirming actually removes the row.
    await user.click(within(row).getByRole("button", { name: "Revoke…" }));
    await user.click(within(row).getByRole("button", { name: "Revoke" }));
    await waitFor(() => expect(within(table).queryByText("RTX-PC")).not.toBeInTheDocument());
  });

  it("pairing flow: naming a machine generates a token and command", async () => {
    renderSettings("/app/settings/machines");
    const user = userEvent.setup();

    await user.click(await screen.findByRole("button", { name: "Pair new machine" }, { timeout: 5000 }));
    expect(screen.getByText("Name the machine")).toBeInTheDocument();

    const nameInput = screen.getByPlaceholderText("e.g. Office PC");
    await user.type(nameInput, "Office PC");
    await user.click(screen.getByRole("button", { name: "Generate pairing token" }));

    expect(await screen.findByText("Run this on Office PC", {}, { timeout: 5000 })).toBeInTheDocument();
    expect(screen.getByText(/sentinal-agent pair/)).toBeInTheDocument();
    expect(screen.getByText(/expires in/)).toBeInTheDocument();
  });
});
