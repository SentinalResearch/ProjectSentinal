import { render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeAll, describe, expect, it } from "vitest";

import { AuthProvider } from "../../context/AuthContext";
import { ThemeProvider } from "../../context/ThemeContext";
import { AppShell } from "./AppShell";
import { ProjectPage } from "./ProjectPage";

function renderProject(id: string) {
  return render(
    <MemoryRouter initialEntries={[`/app/projects/${id}`]}>
      <ThemeProvider>
        <AuthProvider>
          <Routes>
            <Route path="/app" element={<AppShell />}>
              <Route path="projects/:id" element={<ProjectPage />} />
            </Route>
          </Routes>
        </AuthProvider>
      </ThemeProvider>
    </MemoryRouter>,
  );
}

describe("ProjectPage", () => {
  beforeAll(() => {
    Object.defineProperty(window, "innerWidth", { writable: true, configurable: true, value: 1440 });
  });

  it("resolves the project by slug and lists its sessions and event journal", async () => {
    renderProject("tim");
    expect(await screen.findByRole("heading", { name: "TIM" })).toBeInTheDocument();
    // "Qwen Training" appears both as a session row and next to its events in the journal.
    expect((await screen.findAllByText("Qwen Training")).length).toBeGreaterThanOrEqual(2);
    expect((await screen.findAllByText("PROGRESS")).length).toBeGreaterThanOrEqual(1);
  });

  it("shows a not-found state for an unknown project slug", async () => {
    renderProject("does-not-exist");
    expect(await screen.findByText("Project not found.")).toBeInTheDocument();
  });
});
