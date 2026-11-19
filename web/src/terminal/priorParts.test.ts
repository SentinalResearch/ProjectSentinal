import type { Terminal } from "@xterm/xterm";
import { describe, expect, it, vi } from "vitest";

import type { Session } from "../data/types";
import { writePriorParts } from "./priorParts";

function part(id: string, fields: Partial<Session> = {}): Session {
  return { id, machine: "pop", name: "Session 1", state: "INTERRUPTED", exitCode: null, ...fields } as Session;
}

/** Records writes and the line each marker lands on; decorations render into plain elements. */
function fakeTerminal() {
  const writes: string[] = [];
  let line = 0;
  const rendered: HTMLElement[] = [];
  const terminal = {
    cols: 80,
    buffer: { active: { cursorX: 0, cursorY: 0, baseY: 0, type: "normal" as "normal" | "alternate" } },
    write(data: string | Uint8Array, callback?: () => void) {
      const text = typeof data === "string" ? data : new TextDecoder().decode(data);
      writes.push(text);
      line += (text.match(/\n/g) ?? []).length;
      if (text.includes("\x1b[?1049h")) terminal.buffer.active.type = "alternate";
      if (text.includes("\x1b[?1049l")) {
        // Leaving restores the cursor saved on entry, as a real terminal does.
        terminal.buffer.active.type = "normal";
        line = 0;
      }
      terminal.buffer.active.cursorX = text.endsWith("\n") || text === "" ? 0 : 1;
      callback?.();
    },
    registerMarker: () => ({ line, isDisposed: false, dispose() {} }),
    registerDecoration: () => ({
      onRender(render: (element: HTMLElement) => void) {
        const element = document.createElement("div");
        rendered.push(element);
        render(element);
      },
      dispose() {},
    }),
  };
  return { terminal: terminal as unknown as Terminal, writes, rendered };
}

describe("writePriorParts", () => {
  it("writes each earlier part, then a Resumed tag before the next, oldest first", async () => {
    const { terminal, writes, rendered } = fakeTerminal();
    const replay = vi.fn(async (id: string, onChunk: (chunk: Uint8Array) => void) => {
      onChunk(new TextEncoder().encode(`output of ${id}\r\n`));
      return { bytesRead: 10 };
    });
    const parts = [part("pop:1", { state: "COMPLETED", exitCode: 0 }), part("pop:2")];
    const latest = part("pop:3", { state: "RUNNING" });

    const markers = await writePriorParts(terminal, { parts, latest, offset: 0, total: 3, maxBytes: 1000, replay });

    const text = writes.join("");
    expect(text.indexOf("output of pop:1")).toBeLessThan(text.indexOf("output of pop:2"));
    expect(replay).toHaveBeenCalledWith("pop:1", expect.any(Function), { maxBytes: 1000 });
    expect(markers.map((marker) => marker.id)).toEqual(["pop:1", "pop:2", "pop:3"]);
    expect(rendered.map((element) => element.textContent)).toEqual([
      expect.stringContaining("before: exit 0"),
      expect.stringContaining("before: interrupted · exit ?"),
    ]);
    expect(rendered[1].textContent).toContain("part 3 / 3");
    expect(rendered[1].getAttribute("aria-label")).toContain("Part 3 of 3");
    // The tag sits on a blank line: no tag text is ever written into the terminal.
    expect(text).not.toContain("Resumed");
  });

  it("numbers parts after the ones left out of the window", async () => {
    const { terminal, rendered } = fakeTerminal();
    const replay = vi.fn(async () => ({ bytesRead: 0 }));
    await writePriorParts(terminal, {
      parts: [part("pop:9")],
      latest: part("pop:10"),
      offset: 8,
      total: 10,
      maxBytes: 1,
      replay,
    });
    expect(rendered[0].textContent).toContain("part 10 / 10");
  });

  it("keeps going past a part whose saved output cannot be read", async () => {
    const { terminal, writes, rendered } = fakeTerminal();
    const replay = vi.fn(async (id: string, onChunk: (chunk: Uint8Array) => void) => {
      if (id === "pop:1") throw new Error("log missing");
      onChunk(new TextEncoder().encode("second\r\n"));
      return { bytesRead: 8 };
    });
    await writePriorParts(terminal, {
      parts: [part("pop:1"), part("pop:2")],
      latest: part("pop:3"),
      offset: 0,
      total: 3,
      maxBytes: 100,
      replay,
    });
    const text = writes.join("");
    expect(text).toContain("Saved output of this part could not be loaded: log missing");
    expect(text).toContain("second");
    expect(rendered).toHaveLength(2);
  });

  it("stops writing once cancelled", async () => {
    const { terminal, writes } = fakeTerminal();
    let cancelled = false;
    const replay = vi.fn(async (_id: string, onChunk: (chunk: Uint8Array) => void) => {
      onChunk(new TextEncoder().encode("first\r\n"));
      cancelled = true;
      return { bytesRead: 7 };
    });
    await writePriorParts(terminal, {
      parts: [part("pop:1"), part("pop:2")],
      latest: part("pop:3"),
      offset: 0,
      total: 3,
      maxBytes: 100,
      replay,
      cancelled: () => cancelled,
    });
    expect(replay).toHaveBeenCalledTimes(1);
    expect(writes.join("")).not.toContain("second");
  });

  it("puts each tag on its own line, leaving full-screen mode only when a part ended in it", async () => {
    const { terminal, writes } = fakeTerminal();
    const replay = vi.fn(async (id: string, onChunk: (chunk: Uint8Array) => void) => {
      if (id === "pop:2") onChunk(new TextEncoder().encode("\x1b[?1049hvim"));
      return { bytesRead: 1 };
    });

    const markers = await writePriorParts(terminal, {
      parts: [part("pop:1"), part("pop:2")],
      latest: part("pop:3"),
      offset: 0,
      total: 3,
      maxBytes: 100,
      replay,
    });

    expect(writes.filter((text) => text.includes("\x1b[?1049l"))).toHaveLength(1);
    const lines = markers.slice(1).map((entry) => entry.marker.line);
    expect(new Set(lines).size).toBe(lines.length);
  });
});
