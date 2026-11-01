/** Mock search (5.S): the fixture store searched with the daemon's ranking. */
import type { SearchQuery, SearchResult, SearchResults } from "../search";
import { clip, matchScore, mergeResults, rankScore } from "../search";
import type { Session } from "../types";
import { store } from "./store";

const DAY = 86_400_000;

function outcomeOf(session: Session): "FAILED" | "SUCCEEDED" | "RUNNING" | undefined {
  if (session.state === "FAILED") return "FAILED";
  if (session.state === "COMPLETED") return "SUCCEEDED";
  if (session.state === "RUNNING" || session.state === "WAITING" || session.state === "STALLED") return "RUNNING";
  return undefined;
}

export function mockSearch(query: SearchQuery, now: number = Date.now()): SearchResults {
  const limit = Math.min(query.limit ?? 50, 50);
  const wants = (scope: SearchResult["kind"]) => !query.scopes?.length || query.scopes.includes(scope as never);
  const onMachine = (machine: string | undefined) => !query.machine || machine === query.machine;
  const results: SearchResult[] = [];
  const add = (result: Omit<SearchResult, "score">, matched: number, at: number | undefined, needsYou = false) => {
    if (query.since !== undefined && (at === undefined || at < query.since)) return;
    const score = rankScore(matched, at === undefined ? Number.MAX_SAFE_INTEGER : now - at, needsYou);
    if (score > 0) results.push({ ...result, title: clip(result.title, 160), detail: clip(result.detail, 200), score, at });
  };
  for (const session of store.sessions) {
    if (!onMachine(session.machine) || (query.outcome && outcomeOf(session) !== query.outcome)) continue;
    const at = session.updatedAt ?? now;
    const to = `/app/sessions/${encodeURIComponent(session.id)}`;
    if (wants("SESSIONS")) {
      add(
        { id: `session:${session.id}`, kind: "SESSIONS", title: session.name, detail: session.command ?? "", machineId: session.machine, to, sessionId: session.id },
        matchScore(query.text, [session.name, session.command ?? ""]),
        at,
        session.state === "WAITING",
      );
    }
    if (wants("EVENTS") && query.outcome !== "SUCCEEDED" && query.outcome !== "RUNNING") {
      (session.events ?? []).forEach(([, type, detail], index) => {
        if (query.outcome === "FAILED" && type !== "ERROR") return;
        add(
          { id: `event:${session.id}:${index}`, kind: "EVENTS", title: `${type.charAt(0)}${type.slice(1).toLowerCase()} · ${session.name}`, detail, machineId: session.machine, to, sessionId: session.id },
          detail ? matchScore(query.text, [detail]) : 0,
          at,
        );
      });
    }
  }
  if (wants("COMMANDS")) {
    for (const entry of store.history) {
      if (!onMachine(entry.machine)) continue;
      const outcome = entry.ok === null ? "RUNNING" : entry.ok ? "SUCCEEDED" : "FAILED";
      if (query.outcome && outcome !== query.outcome) continue;
      add(
        {
          id: `command:${entry.id}`,
          kind: "COMMANDS",
          title: entry.command,
          detail: entry.duration,
          machineId: entry.machine ?? "",
          to: entry.sessionId ? `/app/sessions/${encodeURIComponent(entry.sessionId)}` : `/app/history?q=${encodeURIComponent(entry.command)}`,
          sessionId: entry.sessionId,
        },
        matchScore(query.text, [entry.command]),
        now - (entry.daysAgo ?? 0) * DAY,
      );
    }
  }
  if (wants("RECIPES") && !query.outcome && query.since === undefined) {
    for (const recipe of store.recipes) {
      if (!onMachine(recipe.machine)) continue;
      add(
        { id: `recipe:${recipe.id}`, kind: "RECIPES", title: recipe.name, detail: recipe.template, machineId: recipe.machine, to: `/app/recipes/${encodeURIComponent(recipe.id)}` },
        matchScore(query.text, [recipe.name, recipe.id, recipe.category, recipe.template]),
        undefined,
      );
    }
  }
  return mergeResults([results], limit);
}

/** Mock completion (5.A): fixture history lines that start with the prefix, most used first. */
export function mockComplete(prefix: string, limit = 20): import("../../protocol/Completion").Completion[] {
  const counts = new Map<string, number>();
  for (const entry of store.history) {
    if (entry.ok !== false && entry.command.startsWith(prefix) && entry.command !== prefix && !entry.command.includes("\n")) {
      counts.set(entry.command, (counts.get(entry.command) ?? 0) + 1);
    }
  }
  return Array.from(counts, ([text, count]) => ({
    text,
    source: "HISTORY" as const,
    count,
    last_at: null,
    last_exit: 0,
    score: count,
  }))
    .sort((left, right) => right.score - left.score || left.text.localeCompare(right.text))
    .slice(0, limit);
}
