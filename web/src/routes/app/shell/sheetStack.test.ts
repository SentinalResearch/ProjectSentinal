import { describe, expect, it } from "vitest";

import { nextStack, type SheetStack } from "./sheetStack";

function counter() {
  let next = 0;
  return () => `f${(next += 1)}`;
}

const at = (pathname: string, search = "") => ({ pathname, search });

describe("back-history sheets", () => {
  it("pushes the page you leave behind, newest first, keeping three", () => {
    const key = counter();
    let stack: SheetStack = { current: { key: key(), ...at("/app/overview") }, sheets: [] };
    for (const path of ["/app/history", "/app/recipes", "/app/split", "/app/settings"]) {
      stack = nextStack(stack, at(path), "PUSH", key);
    }
    expect(stack.current.pathname).toBe("/app/settings");
    expect(stack.sheets.map((sheet) => sheet.pathname)).toEqual(["/app/split", "/app/recipes", "/app/history"]);
  });

  it("restoring a sheet reuses its frame, so its state survives", () => {
    const key = counter();
    let stack: SheetStack = { current: { key: key(), ...at("/app/sessions/rtx%3A1") }, sheets: [] };
    const terminalFrame = stack.current.key;
    stack = nextStack(stack, at("/app/overview"), "PUSH", key);
    stack = nextStack(stack, at("/app/sessions/rtx%3A1"), "PUSH", key);
    expect(stack.current.key).toBe(terminalFrame);
    expect(stack.sheets.map((sheet) => sheet.pathname)).toEqual(["/app/overview"]);
  });

  it("a replace updates the current page in place (no sheet per keystroke)", () => {
    const key = counter();
    let stack: SheetStack = { current: { key: key(), ...at("/app/history") }, sheets: [] };
    stack = nextStack(stack, at("/app/history", "?q=n"), "REPLACE", key);
    stack = nextStack(stack, at("/app/history", "?q=npm"), "REPLACE", key);
    expect(stack.current).toEqual({ key: "f1", pathname: "/app/history", search: "?q=npm" });
    expect(stack.sheets).toEqual([]);
  });

  it("moving within one page updates it in place: Settings tabs, recipe list → form", () => {
    const key = counter();
    let stack: SheetStack = { current: { key: key(), ...at("/app/settings") }, sheets: [] };
    stack = nextStack(stack, at("/app/settings/machines"), "PUSH", key);
    stack = nextStack(stack, at("/app/settings/account"), "PUSH", key);
    expect(stack).toEqual({ current: { key: "f1", pathname: "/app/settings/account", search: "" }, sheets: [] });
    stack = nextStack(stack, at("/app/recipes"), "PUSH", key);
    stack = nextStack(stack, at("/app/recipes/qwen-vllm"), "PUSH", key);
    expect(stack.sheets.map((sheet) => sheet.pathname)).toEqual(["/app/settings/account"]);
    expect(stack.current.key).toBe("f2");
  });

  it("different Sessions are different pages", () => {
    const key = counter();
    let stack: SheetStack = { current: { key: key(), ...at("/app/sessions/rtx%3A1") }, sheets: [] };
    stack = nextStack(stack, at("/app/sessions/rtx%3A2"), "PUSH", key);
    expect(stack.sheets.map((sheet) => sheet.pathname)).toEqual(["/app/sessions/rtx%3A1"]);
  });
});
