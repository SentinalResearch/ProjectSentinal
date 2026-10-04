import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import { AuthProvider } from "../../context/AuthContext";
import { ThemeProvider } from "../../context/ThemeContext";
import { dataAdapter } from "../../data";
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
  afterEach(() => vi.restoreAllMocks());

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

  it("launches the chosen catalog entry on its own Machine, even with a slash in the composite id", async () => {
    const recipe = (machine: string) => ({
      id: `${machine}/cargo-test`,
      sourceId: "cargo-test",
      name: "cargo test",
      category: "TESTS",
      machine,
      machineIds: [machine],
      template: "cargo test {arguments}",
      params: { arguments: "" },
    });
    vi.spyOn(dataAdapter, "getRecipes").mockResolvedValue([recipe("rtx"), recipe("mini")]);
    const run = vi.spyOn(dataAdapter, "runRecipe").mockResolvedValue({ sessionId: "mini:9" });
    renderRecipes("/app/recipes");
    const user = userEvent.setup();

    await user.click(await screen.findByRole("button", { name: /cargo test.*Mac Mini/ }, { timeout: 5000 }));
    await user.click(await screen.findByRole("button", { name: "Run on Mac Mini" }, { timeout: 5000 }));

    await waitFor(() => expect(run).toHaveBeenCalledWith("mini/cargo-test", { arguments: "" }, "mini"));
    expect(await screen.findByText("session opened")).toBeInTheDocument();
  });

  describe("Save as Recipe", () => {
    const command = "awk '{print $1}' data.csv";
    const newPath = `/app/recipes/new?machine=rtx&command=${encodeURIComponent(command)}&cwd=${encodeURIComponent("~/data")}`;

    it("turns chosen literals into parameters and escapes literal braces", async () => {
      renderRecipes(newPath);
      const user = userEvent.setup();
      const save = vi.spyOn(dataAdapter, "saveRecipe").mockResolvedValue();

      await user.click(await screen.findByRole("checkbox", { name: "data.csv" }, { timeout: 5000 }));
      expect(screen.getByTestId("save-recipe-template")).toHaveTextContent("awk '{{print $1}}' {value1}");
      await user.click(screen.getByRole("button", { name: "Save Recipe" }));

      await waitFor(() => expect(save).toHaveBeenCalledTimes(1));
      expect(save).toHaveBeenCalledWith(
        "rtx",
        expect.objectContaining({
          template: "awk '{{print $1}}' {value1}",
          params: { value1: "data.csv" },
          project: "~/data",
        }),
      );
    });

    it("refuses an ID that is not lowercase words joined by hyphens", async () => {
      renderRecipes(newPath);
      const user = userEvent.setup();
      const id = await screen.findByRole("textbox", { name: "ID" }, { timeout: 5000 });
      await user.clear(id);
      await user.type(id, "../escape");
      expect(screen.getByRole("button", { name: "Save Recipe" })).toBeDisabled();
      await user.clear(id);
      await user.type(id, "awk-first-column");
      expect(screen.getByRole("button", { name: "Save Recipe" })).toBeEnabled();
    });

    it("keeps the form and shows why when the Machine refuses the Recipe", async () => {
      renderRecipes(newPath);
      const user = userEvent.setup();
      vi.spyOn(dataAdapter, "saveRecipe").mockRejectedValue(new Error("recipe id already exists"));

      await user.click(await screen.findByRole("button", { name: "Save Recipe" }, { timeout: 5000 }));
      expect(await screen.findByRole("alert")).toHaveTextContent("recipe id already exists");
      expect(screen.getByRole("button", { name: "Save Recipe" })).toBeEnabled();
    });
  });
});
