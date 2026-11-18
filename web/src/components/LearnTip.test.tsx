import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { LearnTip, noteSearchUsed, resetLearning } from "./LearnTip";

afterEach(() => resetLearning());

describe("LearnTip", () => {
  it("teaches both ways to move around until dismissed", () => {
    render(<LearnTip onTry={() => undefined} />);
    expect(screen.getByRole("complementary", { name: "How to get around" })).toHaveTextContent("go · run · find");
    fireEvent.click(screen.getByRole("button", { name: "Got it" }));
    expect(screen.queryByRole("complementary", { name: "How to get around" })).toBeNull();
  });

  it("goes away once search has been used three times", () => {
    const { container } = render(<LearnTip />);
    act(() => {
      noteSearchUsed();
      noteSearchUsed();
    });
    expect(container).not.toBeEmptyDOMElement();
    act(() => noteSearchUsed());
    expect(screen.queryByRole("complementary")).toBeNull();
  });
});
