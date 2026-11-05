import { act, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, useLocation } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";

import { PlacesMenu } from "./PlacesMenu";

function Where() {
  return <div data-testid="where">{useLocation().pathname}</div>;
}

function renderMenu(onSearch?: () => void) {
  render(
    <MemoryRouter initialEntries={["/app/history"]}>
      <PlacesMenu label="Go somewhere" triggerClass="" onSearch={onSearch}>
        History
      </PlacesMenu>
      <Where />
    </MemoryRouter>,
  );
  return screen.getByRole("button", { name: "Go somewhere" });
}

const menu = () => screen.queryByRole("group", { name: "Places" });

describe("PlacesMenu", () => {
  afterEach(() => vi.useRealTimers());

  it("takes no space until wanted, and hovering drops it down", () => {
    const trigger = renderMenu();
    expect(menu()).not.toBeInTheDocument();
    fireEvent.mouseEnter(trigger.parentElement as HTMLElement);
    const places = within(menu() as HTMLElement);
    for (const name of ["Overview", "Terminals", "Machines", "History", "Recipes", "Settings"]) {
      expect(places.getByRole("link", { name })).toBeInTheDocument();
    }
    expect(places.getByRole("link", { name: "History" })).toHaveAttribute("aria-current", "page");
  });

  it("survives crossing the gap to the menu, and closes after moving away", () => {
    vi.useFakeTimers();
    const root = renderMenu().parentElement as HTMLElement;
    fireEvent.mouseEnter(root);
    fireEvent.mouseMove(document.body);
    act(() => void vi.advanceTimersByTime(100));
    fireEvent.mouseMove(root);
    act(() => void vi.advanceTimersByTime(1000));
    expect(menu()).toBeInTheDocument();
    fireEvent.mouseMove(document.body);
    act(() => void vi.advanceTimersByTime(200));
    expect(menu()).not.toBeInTheDocument();
  });

  it("stays open when the island moves under a still pointer", () => {
    vi.useFakeTimers();
    const root = renderMenu().parentElement as HTMLElement;
    fireEvent.mouseEnter(root);
    // Live data resized the island: a mouse-out with no pointer movement.
    fireEvent.mouseLeave(root);
    act(() => void vi.advanceTimersByTime(1000));
    expect(menu()).toBeInTheDocument();
  });

  it("a tap opens it and a second tap closes it, whatever hover events follow", () => {
    vi.useFakeTimers();
    vi.setSystemTime(10_000);
    const trigger = renderMenu();
    const root = trigger.parentElement as HTMLElement;
    // The order a real phone fires: touch, a synthetic hover, the click, then
    // a phantom mouse-out that must not read as the pointer leaving.
    fireEvent.touchStart(trigger);
    fireEvent.mouseEnter(root);
    fireEvent.click(trigger);
    expect(menu()).toBeInTheDocument();
    fireEvent.mouseLeave(root);
    fireEvent.mouseMove(document.body);
    act(() => void vi.advanceTimersByTime(1000));
    expect(menu()).toBeInTheDocument();

    vi.setSystemTime(13_000);
    fireEvent.touchStart(trigger);
    fireEvent.mouseEnter(root);
    fireEvent.click(trigger);
    expect(menu()).not.toBeInTheDocument();
  });

  it("works from the keyboard, and Escape returns to the trigger", async () => {
    const user = userEvent.setup();
    const trigger = renderMenu();
    trigger.focus();
    await user.keyboard("{Enter}");
    expect(trigger).toHaveAttribute("aria-expanded", "true");
    await user.keyboard("{Escape}");
    expect(menu()).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
  });

  it("choosing a place goes there and closes; Search opens the prompt", async () => {
    const user = userEvent.setup();
    const onSearch = vi.fn();
    const trigger = renderMenu(onSearch);
    await user.click(trigger);
    await user.click(within(menu() as HTMLElement).getByRole("link", { name: "Machines" }));
    expect(screen.getByTestId("where")).toHaveTextContent("/app/settings/machines");
    expect(menu()).not.toBeInTheDocument();
    await user.click(trigger);
    await user.click(within(menu() as HTMLElement).getByRole("button", { name: "Search" }));
    expect(onSearch).toHaveBeenCalledOnce();
    expect(menu()).not.toBeInTheDocument();
  });

  it("closes on a press elsewhere", async () => {
    const user = userEvent.setup();
    await user.click(renderMenu());
    fireEvent.pointerDown(document.body);
    expect(menu()).not.toBeInTheDocument();
  });
});
