import { render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";

vi.mock("../context/AuthContext", () => ({
  useAuth: vi.fn(),
}));

import { useAuth } from "../context/AuthContext";
import { PublicAuthGuard, RequireAuth, safeReturnPath } from "./guards";

const mockedAuth = vi.mocked(useAuth);

describe("notification login return routing", () => {
  it("accepts only bounded internal app routes", () => {
    expect(safeReturnPath("/app/sessions/mac%3A1?view=output")).toBe("/app/sessions/mac%3A1?view=output");
    expect(safeReturnPath("https://evil.example/steal")).toBeNull();
    expect(safeReturnPath("//evil.example/steal")).toBeNull();
    expect(safeReturnPath("/login")).toBeNull();
    expect(safeReturnPath("/app/sessions/a%2F..%2Fadmin")).toBeNull();
    expect(safeReturnPath("/app/sessions/a\\admin")).toBeNull();
    expect(safeReturnPath("/app/overview#secret")).toBeNull();
  });

  it("returns to the namespaced Session after authentication", () => {
    mockedAuth.mockReturnValue({ status: "signedIn", loading: false } as ReturnType<typeof useAuth>);
    render(
      <MemoryRouter initialEntries={[{ pathname: "/login", state: { returnTo: "/app/sessions/linux%3A1" } }]}>
        <Routes>
          <Route element={<PublicAuthGuard />}>
            <Route path="/login" element={<div>login</div>} />
          </Route>
          <Route path="/app/sessions/:id" element={<div>correct Session</div>} />
        </Routes>
      </MemoryRouter>,
    );
    expect(screen.getByText("correct Session")).toBeInTheDocument();
  });

  it("preserves the requested internal route when authentication is required", () => {
    mockedAuth.mockReturnValue({ status: "signedOut", loading: false } as ReturnType<typeof useAuth>);
    render(
      <MemoryRouter initialEntries={["/app/sessions/mac%3A1"]}>
        <Routes>
          <Route element={<RequireAuth />}>
            <Route path="/app/sessions/:id" element={<div>private</div>} />
          </Route>
          <Route path="/login" element={<div>login</div>} />
        </Routes>
      </MemoryRouter>,
    );
    expect(screen.getByText("login")).toBeInTheDocument();
  });
});
