import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";

import { AuthProvider } from "../../context/AuthContext";
import { ThemeProvider } from "../../context/ThemeContext";
import { dataAdapter } from "../../data";
import { config } from "../../data/config";
import { serverHost } from "./AuthIsland";
import { AUTH_COPY } from "./copy";
import { islandFor } from "./LoginPage";
import { StartupPage, startupDestination } from "./StartupPage";

function renderStartup() {
  return render(
    <MemoryRouter initialEntries={["/"]}>
      <ThemeProvider>
        <AuthProvider>
          <Routes>
            <Route path="/" element={<StartupPage />} />
            <Route path="/login" element={<p>login screen</p>} />
            <Route path="/setup" element={<p>setup screen</p>} />
            <Route path="/app/overview" element={<p>overview screen</p>} />
          </Routes>
        </AuthProvider>
      </ThemeProvider>
    </MemoryRouter>,
  );
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("StartupPage", () => {
  it("runs the checks, then hands a signed-out browser to sign-in", async () => {
    vi.spyOn(dataAdapter.auth, "getState").mockResolvedValue({ status: "signedOut", attempts: 0, lockUntil: null });
    renderStartup();

    expect(screen.getByRole("list", { name: "Startup checks" })).toBeInTheDocument();
    expect(screen.getByText("Reaching the server")).toBeInTheDocument();
    expect(await screen.findByText("login screen", {}, { timeout: 2000 })).toBeInTheDocument();
  });

  it("shows the unreachable state and checks again on Try again", async () => {
    const getState = vi
      .spyOn(dataAdapter.auth, "getState")
      .mockRejectedValueOnce(new TypeError("Failed to fetch"))
      .mockResolvedValue({ status: "setup", attempts: 0, lockUntil: null });
    renderStartup();

    expect(await screen.findByRole("heading", { name: "Can't reach the server" })).toBeInTheDocument();
    expect(screen.getByText("no answer")).toBeInTheDocument();
    expect(screen.getByText("Unreachable")).toBeInTheDocument();

    await userEvent.setup().click(screen.getByRole("button", { name: /try again/i }));
    expect(getState).toHaveBeenCalledTimes(2);
    expect(await screen.findByText("setup screen", {}, { timeout: 2000 })).toBeInTheDocument();
  });
});

describe("startupDestination", () => {
  it("sends each auth state to its screen", () => {
    expect(startupDestination("signedIn")).toBe("/app/overview");
    expect(startupDestination("setup")).toBe("/setup");
    expect(startupDestination("signedOut")).toBe("/login");
    expect(startupDestination("expired")).toBe("/login");
  });
});

describe("islandFor", () => {
  it("says what each sign-in state means, never by colour alone", () => {
    const base = { reachable: true, attemptsLeft: 4, clock: null };
    expect(islandFor("login", base)).toEqual({ tone: "ok", status: "Signed out" });
    expect(islandFor("wrong", base)).toEqual({ tone: "failed", status: "4 tries left" });
    expect(islandFor("wrong", { ...base, attemptsLeft: 1 })).toEqual({ tone: "failed", status: "1 try left" });
    expect(islandFor("locked", { ...base, clock: "04:52" })).toEqual({ tone: "failed", status: "Locked · 04:52" });
    expect(islandFor("expired", base)).toEqual({ tone: "waiting", status: "Sign-in expired" });
  });

  it("reports an unreachable server before anything else", () => {
    expect(islandFor("locked", { reachable: false, attemptsLeft: 0, clock: "01:00" })).toEqual({
      tone: "off",
      status: "Unreachable",
    });
  });
});

describe("serverHost", () => {
  it("uses the daemon host when the daemon lives elsewhere", () => {
    expect(serverHost("https://mini.tail.example:7781/api/v1", "app.local")).toBe("mini.tail.example:7781");
  });

  it("falls back to the page host for a relative daemon URL", () => {
    expect(serverHost("/api/v1", "localhost:5173")).toBe("localhost:5173");
  });
});

describe("locked copy", () => {
  it("states the configured attempt limit", () => {
    expect(AUTH_COPY.locked.body).toContain(`after ${config.loginMaxAttempts} incorrect passwords`);
  });
});
