import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { ThemeProvider } from "../context/ThemeContext";
import { ThemeChoice, ThemeSwitch } from "./ThemeSwitch";

describe("ThemeSwitch", () => {
  it("flips between dark and light and names the theme it switches to", async () => {
    localStorage.removeItem("sentinal.theme");
    render(
      <ThemeProvider>
        <ThemeSwitch />
      </ThemeProvider>,
    );
    const user = userEvent.setup();
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");

    await user.click(screen.getByRole("button", { name: "Switch to light theme" }));
    expect(document.documentElement).toHaveAttribute("data-theme", "light");

    await user.click(screen.getByRole("button", { name: "Switch to dark theme" }));
    expect(document.documentElement).toHaveAttribute("data-theme", "dark");
  });

  it("offers Light, Dark and Auto as one choice", async () => {
    localStorage.removeItem("sentinal.theme");
    render(
      <ThemeProvider>
        <ThemeChoice />
      </ThemeProvider>,
    );
    const user = userEvent.setup();
    expect(screen.getByRole("radio", { name: "Dark" })).toHaveAttribute("aria-checked", "true");
    await user.click(screen.getByRole("radio", { name: "Light" }));
    expect(document.documentElement).toHaveAttribute("data-theme", "light");
    expect(screen.getByRole("radio", { name: "Light" })).toHaveAttribute("aria-checked", "true");
    await user.click(screen.getByRole("radio", { name: "Dark" }));
  });

  it("renders nothing outside a ThemeProvider", () => {
    const { container } = render(<ThemeSwitch />);
    expect(container).toBeEmptyDOMElement();
  });
});
