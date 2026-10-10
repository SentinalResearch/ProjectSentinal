import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { dataAdapter } from "../../../data";
import { resetSimulation } from "../../../data/mock/simulation";
import { resetViewport, setViewport } from "../../../test-viewport";
import { HistoryPage } from "../HistoryPage";
import { RecipesPage } from "../RecipesPage";
import { SplitViewPage } from "../SplitViewPage";
import { renderAt } from "./harness";

const routes = [
  { path: "split", element: <SplitViewPage /> },
  { path: "history", element: <HistoryPage /> },
  { path: "recipes", element: <RecipesPage /> },
  { path: "recipes/:id", element: <RecipesPage /> },
];

describe("phone lists", () => {
  beforeEach(() => {
    resetSimulation();
    setViewport(390);
  });
  afterEach(() => {
    resetViewport();
    vi.restoreAllMocks();
  });

  it("C3: stacks live terminal cards that open full screen, with the focused one outlined", async () => {
    renderAt("/app/split?focus=qwen", routes);
    expect(await screen.findByRole("heading", { name: "Terminals" }, { timeout: 5000 })).toBeInTheDocument();
    const card = screen.getByRole("link", { name: /Qwen Training/ });
    expect(card).toHaveAttribute("href", "/app/sessions/qwen/terminal");
    expect(card).toHaveAttribute("data-focused", "true");
    expect(screen.queryByRole("link", { name: /pytest/ })).not.toBeInTheDocument();
    expect(screen.getByText(/open · tap one to go full screen/)).toBeInTheDocument();
  });

  it("D1: rows show a state square, the Command, and Machine · project · duration with a ⋯ menu", async () => {
    renderAt("/app/history", routes);
    const search = await screen.findByRole("searchbox", { name: "Search commands" }, { timeout: 5000 });
    expect(search).toBeInTheDocument();
    const rows = await screen.findAllByRole("listitem", {}, { timeout: 5000 });
    const row = rows[0];
    expect(within(row).getByText(/·/)).toBeInTheDocument();
    await userEvent.setup().click(within(row).getByRole("button", { name: /More actions/ }));
    expect(screen.getByRole("menuitem", { name: "Copy" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Open" })).toBeInTheDocument();
  });

  it("D2: the filters sheet groups chips, resets, and shows N results", async () => {
    const getHistory = vi.spyOn(dataAdapter, "getHistory");
    renderAt("/app/history", routes);
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "Filters" }, { timeout: 5000 }));
    const sheet = screen.getByRole("dialog", { name: "Filters" });
    for (const group of ["Machine", "Project", "Result", "Date"]) {
      expect(within(sheet).getByRole("group", { name: group })).toBeInTheDocument();
    }
    await user.click(within(within(sheet).getByRole("group", { name: "Result" })).getByRole("button", { name: "Failed" }));
    await user.click(within(sheet).getByRole("button", { name: /^Show \d+ results?$/ }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(getHistory).toHaveBeenLastCalledWith(expect.objectContaining({ result: "failed" }));
  });

  it("D3: Recipes are grouped by category with template, Machine, and parameter count", async () => {
    renderAt("/app/recipes", routes);
    const row = await screen.findByRole("link", { name: /Qwen vLLM Server/ }, { timeout: 5000 });
    expect(row).toHaveTextContent("vllm serve {model}");
    expect(row).toHaveTextContent(/RTX-PC · 3 parameters/);
  });

  it("D4: runs on the chosen Machine, with offline Machines disabled and validation kept", async () => {
    vi.spyOn(dataAdapter, "getRecipes").mockResolvedValue([
      { id: "train", name: "Train", category: "ML", machine: "rtx", machineIds: ["rtx", "mbp"], template: "python train.py --port {port}", params: { port: "8000" } },
    ]);
    const run = vi.spyOn(dataAdapter, "runRecipe").mockResolvedValue({ sessionId: "rtx:9" });
    renderAt("/app/recipes/train", routes);
    const user = userEvent.setup();
    const target = await screen.findByRole("radiogroup", { name: "Target machine" }, { timeout: 5000 });
    expect(within(target).getByRole("radio", { name: /MacBook/ })).toBeDisabled();
    const port = screen.getByRole("textbox", { name: "port" });
    await user.clear(port);
    await user.type(port, "99999");
    expect(screen.getByRole("button", { name: "Run on RTX-PC" })).toBeDisabled();
    await user.clear(port);
    await user.type(port, "9000");
    await user.click(screen.getByRole("button", { name: "Run on RTX-PC" }));
    expect(run).toHaveBeenCalledWith("train", { port: "9000" }, "rtx");
  });
});
