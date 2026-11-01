/**
 * Search everywhere (Milestone 5.S) in the browser: the ranking shared with
 * the daemon (`crates/protocol/src/search.rs`, same fixture table), the `find`
 * grammar, and merging every Machine's hits into one list. Each Machine
 * searches its own store; the browser only merges and ranks what it holds.
 */
import type { SearchHit } from "../protocol/SearchHit";
import type { SearchOutcome } from "../protocol/SearchOutcome";
import type { SearchScope } from "../protocol/SearchScope";
import { slugifyProject } from "./format";
import type { Machine } from "./types";

export const MATCH_PREFIX = 100;
export const MATCH_WORD_START = 80;
export const MATCH_SUBSTRING = 60;
export const MATCH_INITIALS = 40;
export const MATCH_FUZZY = 20;
export const RECENCY_BOOST = 15;
export const RECENCY_HALF_LIFE_MS = 86_400_000;
export const NEEDS_YOU_BOOST = 30;

const isWordChar = (char: string | undefined) => char !== undefined && /[a-z0-9]/.test(char);

function isInitials(needle: string, hay: string): boolean {
  const wanted = Array.from(needle);
  let at = 0;
  let previous: string | undefined;
  for (const char of hay) {
    if (!isWordChar(previous) && char === wanted[at]) at += 1;
    if (at === wanted.length) return true;
    previous = char;
  }
  return false;
}

function isSubsequence(needle: string, hay: string): boolean {
  const wanted = Array.from(needle);
  let at = 0;
  for (const char of hay) {
    if (char === wanted[at]) at += 1;
    if (at === wanted.length) return true;
  }
  return at === wanted.length;
}

function wordScore(word: string, hay: string): number {
  const at = hay.indexOf(word);
  if (at === 0) return MATCH_PREFIX;
  if (at > 0) return isWordChar(Array.from(hay.slice(0, at)).at(-1)) ? MATCH_SUBSTRING : MATCH_WORD_START;
  if (isInitials(word, hay)) return MATCH_INITIALS;
  return isSubsequence(word, hay) ? MATCH_FUZZY : 0;
}

/**
 * How well `query` matches any of `haystacks` (0 = not at all): the mean over
 * the query's words of the best tier each reaches. An empty query scores 1.
 */
export function matchScore(query: string, haystacks: string[]): number {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return 1;
  const hays = haystacks.map((hay) => hay.toLowerCase());
  let total = 0;
  for (const word of words) {
    const best = Math.max(0, ...hays.map((hay) => wordScore(word, hay)));
    if (best === 0) return 0;
    total += best;
  }
  return total / words.length;
}

/** The match plus recency and needs-you boosts, rounded to hundredths like the daemon. */
export function rankScore(matched: number, ageMs: number, needsYou: boolean): number {
  if (matched <= 0) return 0;
  const recency = RECENCY_BOOST * 0.5 ** (Math.max(0, ageMs) / RECENCY_HALF_LIFE_MS);
  return Math.round((matched + recency + (needsYou ? NEEDS_YOU_BOOST : 0)) * 100) / 100;
}

/** One display line of at most `max` characters, ending in `…` when cut. */
export function clip(text: string, max: number): string {
  const line = text.split(/\s+/).filter(Boolean).join(" ");
  const chars = Array.from(line);
  return chars.length <= max ? line : `${chars.slice(0, Math.max(0, max - 1)).join("")}…`;
}

// ── find grammar ────────────────────────────────────────────────────────────

export interface FindQuery {
  /** What is left to match once filter words are read. */
  text: string;
  outcome?: SearchOutcome;
  /** Only this Machine is asked. */
  machine?: string;
  /** Epoch milliseconds. */
  since?: number;
}

const OUTCOMES: Record<string, SearchOutcome> = {
  failed: "FAILED",
  failing: "FAILED",
  errors: "FAILED",
  succeeded: "SUCCEEDED",
  passed: "SUCCEEDED",
  ok: "SUCCEEDED",
  running: "RUNNING",
};

const HOUR = 3_600_000;

function sinceOf(word: string, now: number): number | undefined {
  const midnight = (daysAgo: number) => {
    const day = new Date(now);
    return new Date(day.getFullYear(), day.getMonth(), day.getDate() - daysAgo).getTime();
  };
  if (word === "today") return midnight(0);
  if (word === "yesterday") return midnight(1);
  if (word === "hour") return now - HOUR;
  if (word === "week") return now - 7 * 24 * HOUR;
  const amount = word.match(/^(\d{1,4})([hdw])$/);
  if (!amount) return undefined;
  const unit = { h: HOUR, d: 24 * HOUR, w: 7 * 24 * HOUR }[amount[2] as "h" | "d" | "w"];
  return now - Number(amount[1]) * unit;
}

/** `failed pytest on rtx-pc since yesterday` → text, outcome, Machine, and time. */
export function parseFind(input: string, machines: Machine[], now: number = Date.now()): FindQuery {
  const words = input.trim().split(/\s+/).filter(Boolean);
  const query: FindQuery = { text: "" };
  const rest: string[] = [];
  for (let index = 0; index < words.length; index += 1) {
    const word = words[index];
    const lower = word.toLowerCase();
    const next = words[index + 1]?.toLowerCase();
    if (OUTCOMES[lower] && query.outcome === undefined) {
      query.outcome = OUTCOMES[lower];
      continue;
    }
    if (lower === "on" && next !== undefined) {
      const machine = machines.find((candidate) => candidate.name.toLowerCase() === next || slugifyProject(candidate.name) === next);
      if (machine) {
        query.machine = machine.id;
        index += 1;
        continue;
      }
    }
    if (lower === "since" && next !== undefined) {
      const since = sinceOf(next, now);
      if (since !== undefined) {
        query.since = since;
        index += 1;
        continue;
      }
    }
    rest.push(word);
  }
  query.text = rest.join(" ");
  return query;
}

// ── results ─────────────────────────────────────────────────────────────────

export interface SearchQuery extends FindQuery {
  scopes?: SearchScope[];
  /** Hits wanted in all (and from each Machine). */
  limit?: number;
}

export interface SearchResult {
  id: string;
  kind: SearchScope | "MACHINES" | "PAGES";
  title: string;
  detail: string;
  score: number;
  at?: number;
  machineId: string;
  /** The app route the result opens. */
  to: string;
  /** Composite Session ID, for opening in split view. */
  sessionId?: string;
}

export interface SearchResults extends Array<SearchResult> {
  /** Machines that could not be asked. */
  unavailableMachines?: string[];
}

/** A Machine's hit as a browser result with a route. */
export function resultFromHit(machineId: string, hit: SearchHit): SearchResult {
  const target = hit.target;
  const base = { kind: hit.kind, title: hit.title, detail: hit.detail, score: hit.score, at: hit.at ?? undefined, machineId };
  if (target.type === "RECIPE") {
    return {
      ...base,
      id: `${machineId}:RECIPES:${target.recipe_id}`,
      to: `/app/recipes/${encodeURIComponent(target.recipe_id)}?machine=${encodeURIComponent(machineId)}`,
    };
  }
  const sessionId = `${machineId}:${target.session_id}`;
  const own = target.type === "COMMAND" ? target.command_id : target.type === "EVENT" ? target.event_id : target.session_id;
  return {
    ...base,
    id: `${machineId}:${hit.kind}:${own}`,
    sessionId,
    to: `/app/sessions/${encodeURIComponent(sessionId)}`,
  };
}

/** Every Machine's results as one list, best first, newest first on a tie. */
export function mergeResults(lists: SearchResult[][], limit: number): SearchResult[] {
  return lists
    .flat()
    .sort((left, right) => right.score - left.score || (right.at ?? 0) - (left.at ?? 0))
    .slice(0, limit);
}
