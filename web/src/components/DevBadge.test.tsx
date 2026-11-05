import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { appTitle, DevBadge, isDevEnvironment } from "./DevBadge";

describe("the dev environment is unmistakable", () => {
  it("shows a badge and title only in dev", () => {
    expect(isDevEnvironment("development")).toBe(true);
    expect(isDevEnvironment("production")).toBe(false);
    expect(isDevEnvironment(undefined)).toBe(false);
    expect(appTitle(true)).toBe("Sentinal DEV");
    expect(appTitle(false)).toBe("Sentinal");
    const { unmount } = render(<DevBadge dev />);
    expect(screen.getByText("DEV")).toBeInTheDocument();
    unmount();
    render(<DevBadge dev={false} />);
    expect(screen.queryByText("DEV")).not.toBeInTheDocument();
  });
});
