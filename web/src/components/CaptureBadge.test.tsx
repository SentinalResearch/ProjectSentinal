import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { CaptureLevel } from "../protocol/CaptureLevel";
import type { SessionOrigin } from "../protocol/SessionOrigin";
import { CaptureBadge } from "./CaptureBadge";

const CASES: [SessionOrigin | undefined, CaptureLevel, string, string][] = [
  [undefined, "FULL", "UI·FULL", "Terminal and Commands captured."],
  ["WATCH", "FULL", "WATCH·FULL", "Terminal and Commands captured."],
  ["AUTO_WRAP", "FULL", "AUTO-WRAP·FULL", "Terminal and Commands captured."],
  ["SHELL_HOOK", "COMMANDS", "SHELL HOOK·COMMANDS", "Commands and Metrics only — no terminal."],
  ["ADOPTED", "PROCESS_LOG", "ADOPTED·PROCESS LOG", "Read-only log and Metrics — no terminal input."],
  ["ADOPTED", "PROCESS", "ADOPTED·PROCESS", "Metrics only."],
];

describe("CaptureBadge", () => {
  it.each(CASES)("shows origin %s with capture %s", (origin, capture, text, explanation) => {
    render(<CaptureBadge capture={capture} origin={origin} />);
    const badge = screen.getByLabelText(`${capture} capture`);
    expect(badge).toHaveTextContent(text);
    expect(badge).toHaveAttribute("title", expect.stringContaining(explanation));
  });
});
