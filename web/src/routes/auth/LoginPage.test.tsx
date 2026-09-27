import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { beforeAll, describe, expect, it } from "vitest";

import { AuthProvider } from "../../context/AuthContext";
import { ThemeProvider } from "../../context/ThemeContext";
import { dataAdapter } from "../../data";
import { LoginPage } from "./LoginPage";

function renderLogin() {
  return render(
    <MemoryRouter>
      <ThemeProvider>
        <AuthProvider>
          <LoginPage />
        </AuthProvider>
      </ThemeProvider>
    </MemoryRouter>,
  );
}

describe("LoginPage", () => {
  beforeAll(async () => {
    // First-run: give the mock store a real password so login() has something to check against.
    await dataAdapter.auth.setup("mock-bootstrap", "a-very-long-admin-password");
    await dataAdapter.auth.logout();
  });

  it("shows the wrong-password error and remaining attempts after an incorrect submit", async () => {
    renderLogin();
    const user = userEvent.setup();

    const input = await screen.findByLabelText("Password");
    await user.type(input, "nope");
    await user.click(screen.getByRole("button", { name: /sign in/i }));

    await waitFor(
      () => expect(screen.getByText(/incorrect password/i)).toBeInTheDocument(),
      { timeout: 3000 },
    );
    expect(screen.getByText(/4 attempts left/i)).toBeInTheDocument();
  });
});
