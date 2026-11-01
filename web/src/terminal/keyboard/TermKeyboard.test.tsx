import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { TermKeyboard } from "./TermKeyboard";

function setup(props: Partial<Parameters<typeof TermKeyboard>[0]> = {}) {
  const sent: string[] = [];
  const onBytes = vi.fn((bytes: string) => sent.push(bytes));
  render(<TermKeyboard onBytes={onBytes} {...props} />);
  return { sent, onBytes, user: userEvent.setup(), keys: () => screen.getByRole("group", { name: "Terminal keyboard" }) };
}

describe("built-in terminal keyboard (MOBILE_GLASS_SPEC §4)", () => {
  it("types the acceptance line `tail -f train.log 2>&1 | grep loss⏎` with snippets", async () => {
    const { sent, user } = setup();
    const press = async (name: string) => user.click(screen.getAllByRole("button", { name })[0]);
    for (const char of "tail") await press(char);
    await press("Space");
    await press("-");
    await press("f");
    await press("Space");
    for (const char of "train") await press(char);
    await press("Symbols");
    await press(".");
    await press("Letters");
    for (const char of "log") await press(char);
    await press("Space");
    await press("Symbols");
    await press("2>&1");
    await press("Space");
    await press("| grep");
    await press("Letters");
    for (const char of "loss") await press(char);
    await press("Enter");
    expect(sent.join("")).toBe("tail -f train.log 2>&1 | grep loss\r");
  });

  it("latches Ctrl for one key, shows the named Ctrl layer, and sends ^C as 0x03", async () => {
    const { sent, user } = setup();
    await user.click(screen.getByRole("button", { name: "Ctrl" }));
    expect(screen.getByRole("status")).toHaveTextContent("Ctrl is on");
    const grid = screen.getByRole("group", { name: "Ctrl combinations" });
    expect(within(grid).getByRole("button", { name: "Control R, search hist" })).toBeInTheDocument();
    await user.click(within(grid).getByRole("button", { name: "Control C, interrupt" }));
    expect(sent).toEqual(["\x03"]);
    // The latch is used up: back to letters.
    expect(screen.queryByRole("group", { name: "Ctrl combinations" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "c" }));
    expect(sent).toEqual(["\x03", "c"]);
  });

  it("Ctrl then any letter (via abc) sends Ctrl + that letter", async () => {
    const { sent, user } = setup();
    await user.click(screen.getByRole("button", { name: "Ctrl" }));
    await user.click(screen.getByRole("button", { name: "Letters" }));
    await user.click(screen.getByRole("button", { name: "x" }));
    expect(sent).toEqual(["\x18"]);
  });

  it("a double-tap locks Ctrl until it is tapped again", async () => {
    const { sent, user } = setup();
    const ctrl = screen.getByRole("button", { name: "Ctrl" });
    await user.dblClick(ctrl);
    expect(ctrl).toHaveTextContent("locked");
    const grid = () => screen.getByRole("group", { name: "Ctrl combinations" });
    await user.click(within(grid()).getByRole("button", { name: /Control L/ }));
    await user.click(within(grid()).getByRole("button", { name: /Control D/ }));
    expect(sent).toEqual(["\x0c", "\x04"]);
    await user.click(ctrl);
    expect(screen.queryByRole("group", { name: "Ctrl combinations" })).not.toBeInTheDocument();
  });

  it("sends arrows, Esc, Tab and Alt as their escape sequences", async () => {
    const { sent, user } = setup();
    await user.click(screen.getByRole("button", { name: "Up" }));
    await user.click(screen.getByRole("button", { name: "Left" }));
    await user.click(screen.getByRole("button", { name: "Esc" }));
    await user.click(screen.getByRole("button", { name: "Tab" }));
    await user.click(screen.getByRole("button", { name: "Alt" }));
    await user.click(screen.getByRole("button", { name: "b" }));
    expect(sent).toEqual(["\x1b[A", "\x1b[D", "\x1b", "\t", "\x1bb"]);
  });

  it("is lowercase by default; ⇧ is one-shot and shifts the number row", async () => {
    const { sent, user } = setup();
    await user.click(screen.getByRole("button", { name: "Shift" }));
    await user.click(screen.getByRole("button", { name: "A" }));
    await user.click(screen.getByRole("button", { name: "a" }));
    await user.click(screen.getByRole("button", { name: "Shift" }));
    await user.click(screen.getByRole("button", { name: "!" }));
    expect(sent).toEqual(["A", "a", "!"]);
  });

  it("chips insert their text without Enter; the symbols layer has its own", async () => {
    const { sent, user } = setup({
      chips: [{ label: "↑ python train.py", text: "python train.py", accent: true }],
      symbolChips: [{ label: "loss", text: "loss" }],
    });
    await user.click(screen.getByRole("button", { name: "↑ python train.py" }));
    await user.click(screen.getByRole("button", { name: "Symbols" }));
    await user.click(screen.getByRole("button", { name: "loss" }));
    expect(sent).toEqual(["python train.py", "loss"]);
  });

  it("with a hardware keyboard shows only the accessory row", () => {
    setup({ accessoryOnly: true });
    expect(screen.getByRole("button", { name: "Ctrl" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "q" })).not.toBeInTheDocument();
  });

  it("the prompt's keyboard leaves out the accessory row", () => {
    setup({ accessory: false });
    expect(screen.queryByRole("button", { name: "Ctrl" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "q" })).toBeInTheDocument();
  });
});
