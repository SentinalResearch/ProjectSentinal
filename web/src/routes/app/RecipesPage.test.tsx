import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeAll, describe, expect, it } from "vitest";

import { AuthProvider } from "../../context/AuthContext";
import { ThemeProvider } from "../../context/ThemeContext";
import { AppShell } from "./AppShell";
import { RecipesPage } from "./RecipesPage";

function renderRecipes(path: string) {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <ThemeProvider>
        <AuthProvider>
          <Routes>
            <Route path="/app" element={<AppShell />}>
              <Route path="recipes" element={<RecipesPage />} />
              <Route path="recipes/:id" element={<RecipesPage />} />
              <Route path="sessions/:id" element={<div>session opened</div>} />
            </Route>
          </Routes>
        </AuthProvider>
      </ThemeProvider>
    </MemoryRouter>,
  );
}

describe("RecipesPage", () => {
  beforeAll(() => {
    Object.defineProperty(window, "innerWidth", { writable: true, configurable: true, value: 1440 });
  });

  it("prefills defaults, previews the command, and warns that :8000 is already in use on RTX-PC", async () => {
    renderRecipes("/app/recipes");

    expect(
      await screen.findByRole("heading", { name: "Qwen vLLM Server" }, { timeout: 5000 }),
    ).toBeInTheDocument();
    expect(screen.getByDisplayValue("Qwen/Qwen3-8B")).toBeInTheDocument();
    expect(screen.getByDisplayValue("8000")).toBeInTheDocument();
    expect(screen.getByText(/vllm serve Qwen\/Qwen3-8B --port 8000/)).toBeInTheDocument();
    // The live `vllm` session already uses --port 8000 on rtx (the default target machine).
    expect(await screen.findByText(/already used on RTX-PC/, {}, { timeout: 5000 })).toBeInTheDocument();
  });

  it("blocks the run button with an invalid port and clears the error once fixed", async () => {
    renderRecipes("/app/recipes");
    const user = userEvent.setup();

    const portInput = await screen.findByDisplayValue("8000", {}, { timeout: 5000 });
    await user.clear(portInput);
    await user.type(portInput, "999999");

    expect(await screen.findByText("Port must be 1–65535", {}, { timeout: 5000 })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Run on/ })).toBeDisabled();

    await user.clear(portInput);
    await user.type(portInput, "9000");
    await waitFor(() => expect(screen.queryByText("Port must be 1–65535")).not.toBeInTheDocument());
    expect(screen.getByRole("button", { name: /Run on/ })).toBeEnabled();
  });

  it("runs the recipe and opens the new session", async () => {
    renderRecipes("/app/recipes");
    const user = userEvent.setup();

    await screen.findByRole("heading", { name: "Qwen vLLM Server" }, { timeout: 5000 });
    await user.click(await screen.findByRole("button", { name: /Run on RTX-PC/ }, { timeout: 5000 }));

    expect(await screen.findByText("session opened", {}, { timeout: 5000 })).toBeInTheDocument();
  });
});
