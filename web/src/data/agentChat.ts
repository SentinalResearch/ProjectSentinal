import { useEffect, useState } from "react";

import type { ChatEntry } from "../protocol/ChatEntry";
import type { AgentChat } from "./adapter";
import { dataAdapter } from "./index";

/** Most entries a viewer keeps, as the daemon does. */
export const MAX_CHAT_ENTRIES = 500;

/** Plan bookkeeping: shown as "step N of M", not as cards. */
const PLAN_TOOLS = new Set(["TaskCreate", "TaskUpdate", "TaskList", "TaskGet", "TodoWrite", "ToolSearch"]);

/** One row of the chat view. */
export type ChatRow =
  | { kind: "user"; key: string; at: number; text: string }
  | { kind: "agent"; key: string; at: number; text: string }
  | {
      kind: "tool";
      key: string;
      at: number;
      tool: string;
      summary: string;
      /** Absent while the call is still running. */
      result?: { ok: boolean; preview: string };
    }
  | { kind: "turn"; key: string; at: number; toolUses: number };

/** Entries as rows: each tool call with its result, plan bookkeeping left out. */
export function chatRows(entries: ChatEntry[]): ChatRow[] {
  const rows: ChatRow[] = [];
  const tools = new Map<string, number>();
  const hidden = new Set<string>();
  entries.forEach((entry, index) => {
    const at = Number(entry.at);
    const item = entry.item;
    const key = `${index}`;
    switch (item.kind) {
      case "USER":
        rows.push({ kind: "user", key, at, text: item.text });
        break;
      case "AGENT":
        rows.push({ kind: "agent", key, at, text: item.text });
        break;
      case "TOOL":
        if (PLAN_TOOLS.has(item.tool)) {
          hidden.add(item.id);
          break;
        }
        tools.set(item.id, rows.length);
        rows.push({ kind: "tool", key, at, tool: item.tool, summary: item.summary });
        break;
      case "TOOL_RESULT": {
        if (hidden.has(item.id)) break;
        const at_ = tools.get(item.id);
        const row = at_ === undefined ? undefined : rows[at_];
        if (row?.kind === "tool") row.result = { ok: item.ok, preview: item.preview };
        break;
      }
      case "TURN_FINISHED":
        rows.push({ kind: "turn", key, at, toolUses: item.tool_uses });
        break;
    }
  });
  return rows;
}

/** A reply's inline Markdown: `code` and **bold**; everything else is plain text. */
export type InlinePart = { kind: "text" | "code" | "bold"; text: string };

export function inlineMarkdown(text: string): InlinePart[] {
  const parts: InlinePart[] = [];
  const pattern = /`([^`\n]+)`|\*\*([^*\n]+)\*\*/g;
  let last = 0;
  for (const match of text.matchAll(pattern)) {
    const at = match.index ?? 0;
    if (at > last) parts.push({ kind: "text", text: text.slice(last, at) });
    parts.push(match[1] !== undefined ? { kind: "code", text: match[1] } : { kind: "bold", text: match[2] });
    last = at + match[0].length;
  }
  if (last < text.length) parts.push({ kind: "text", text: text.slice(last) });
  return parts;
}

/** "Ran", "Edited", "Read", … for a tool card. */
export function toolVerb(tool: string): string {
  switch (tool) {
    case "Bash":
    case "PowerShell":
      return "Ran";
    case "Edit":
    case "MultiEdit":
    case "NotebookEdit":
      return "Edited";
    case "Write":
      return "Wrote";
    case "Read":
      return "Read";
    case "Grep":
    case "Glob":
    case "WebSearch":
      return "Searched";
    case "WebFetch":
      return "Fetched";
    case "Task":
    case "Agent":
      return "Delegated";
    default:
      return tool;
  }
}

/** A chat update that arrived on a terminal attach. */
export interface ChatUpdate {
  sessionId: string;
  linked: boolean;
  fromStart: boolean;
  entries: ChatEntry[];
}

const chatListeners = new Set<(update: ChatUpdate) => void>();

/**
 * New chat entries from an attached terminal's socket. The daemon sends a
 * conversation only to viewers attached to that Session, like its output,
 * never to every client.
 */
export function publishChat(update: ChatUpdate): void {
  for (const listener of chatListeners) listener(update);
}

function applyUpdate(previous: AgentChat, update: Omit<ChatUpdate, "sessionId">): AgentChat {
  return {
    linked: update.linked,
    entries: update.fromStart
      ? update.entries
      : [...previous.entries, ...update.entries].slice(-MAX_CHAT_ENTRIES),
  };
}

/** The agent Session's chat, kept current from the owning Machine. */
export function useAgentChat(sessionId: string): AgentChat {
  const [chat, setChat] = useState<AgentChat>({ linked: false, entries: [] });
  useEffect(() => {
    let cancelled = false;
    setChat({ linked: false, entries: [] });
    void dataAdapter.getChat(sessionId).then((reply) => {
      if (!cancelled) setChat(reply);
    });
    const unsubscribe = dataAdapter.subscribe((event) => {
      if (event.type !== "CHAT" || event.sessionId !== sessionId) return;
      setChat((previous) => applyUpdate(previous, event));
    });
    const fromTerminal = (update: ChatUpdate) => {
      if (update.sessionId === sessionId) setChat((previous) => applyUpdate(previous, update));
    };
    chatListeners.add(fromTerminal);
    return () => {
      cancelled = true;
      unsubscribe();
      chatListeners.delete(fromTerminal);
    };
  }, [sessionId]);
  return chat;
}
