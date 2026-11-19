import type { IMarker, Terminal } from "@xterm/xterm";

import type { Session } from "../data/types";
import { renderResumeTag, resumeTag } from "./resumeTags";

/** Where each part of the terminal starts, for jumping to it. */
export interface PartMarker {
  id: string;
  marker: IMarker;
}

type Replay = (
  sessionId: string,
  onChunk: (chunk: Uint8Array) => void,
  options: { maxBytes: number },
) => Promise<unknown>;

export interface PriorPartsPlan {
  /** Earlier parts, oldest first. */
  parts: Session[];
  /** The part the live terminal (or last replay) continues with. */
  latest: Session;
  /** Parts left out before `parts`, for numbering. */
  offset: number;
  total: number;
  maxBytes: number;
  replay: Replay;
  cancelled?: () => boolean;
}

function flush(terminal: Terminal, data: string | Uint8Array): Promise<void> {
  return new Promise((resolve) => terminal.write(data, resolve));
}

/** Reset what the part left on: colours and a hidden cursor. */
const END_OF_PART = "\x1b[0m\x1b[?25h";
/**
 * Leave a full-screen program's screen. Only sent when a part ended on it:
 * leaving also restores a saved cursor, which would move the tag elsewhere.
 */
const LEAVE_FULL_SCREEN = "\x1b[?1049l";

async function writeTag(terminal: Terminal, previous: Session, next: Session, index: number, total: number) {
  if (terminal.buffer.active.type === "alternate") await flush(terminal, LEAVE_FULL_SCREEN);
  const fresh = terminal.buffer.active.cursorX > 0 ? "\r\n" : "";
  await flush(terminal, `${END_OF_PART}${fresh}\r\n`);
  const marker = terminal.registerMarker(0);
  const tag = resumeTag(previous, next, index, total);
  const decoration = marker && terminal.registerDecoration({ marker, width: terminal.cols, height: 1 });
  decoration?.onRender((element) => renderResumeTag(element, tag));
  await flush(terminal, "\r\n\r\n");
  return marker;
}

/**
 * Write every earlier part's saved output into `terminal`, oldest first, with
 * a Resumed tag before each following part. The live attach (or the last
 * part's replay) then continues below, so the whole terminal scrolls as one.
 */
export async function writePriorParts(terminal: Terminal, plan: PriorPartsPlan): Promise<PartMarker[]> {
  const { parts, latest, offset, total, maxBytes, replay } = plan;
  const cancelled = plan.cancelled ?? (() => false);
  const markers: PartMarker[] = [];
  for (const [index, part] of parts.entries()) {
    if (cancelled()) return markers;
    const start = terminal.registerMarker(0);
    if (start && index === 0) markers.push({ id: part.id, marker: start });
    try {
      await replay(part.id, (chunk) => {
        if (!cancelled()) terminal.write(chunk);
      }, { maxBytes });
    } catch (error) {
      const reason = error instanceof Error ? error.message : "unknown error";
      await flush(terminal, `\r\n\x1b[38;2;138;138;134mSaved output of this part could not be loaded: ${reason}\x1b[0m\r\n`);
    }
    if (cancelled()) return markers;
    const next = parts[index + 1] ?? latest;
    const marker = await writeTag(terminal, part, next, offset + index + 1, total);
    if (marker) markers.push({ id: next.id, marker });
  }
  return markers;
}
