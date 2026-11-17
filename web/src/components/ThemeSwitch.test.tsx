import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { ThemeProvider } from "../context/ThemeContext";
import { ThemeSwitch } from "./ThemeSwitch";

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

  it("renders nothing outside a ThemeProvider", () => {
    const { container } = render(<ThemeSwitch />);
    expect(container).toBeEmptyDOMElement();
  });
});
