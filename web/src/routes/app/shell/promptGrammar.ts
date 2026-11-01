/**
 * The prompt's deterministic grammar (GLASS_SPEC.md §5). No model: typed text
 * is matched against what Sentinal already knows. Every suggestion carries the
 * action Enter performs and the text Tab completes to. Nothing here executes:
 * `run` opens the prefilled form and `kill` asks for the usual confirmation.
 */
import { canAnswer, canInterrupt } from "../../../data/actions";
import { needsAttention, slugifyProject } from "../../../data/format";
import { matchScore, type SearchResult } from "../../../data/search";
import type { Machine, Recipe, Session } from "../../../data/types";
import type { SettingsTab } from "../settings-tabs";

export type PromptGroup = "ANSWER" | "GO" | "RUN" | "FIND" | "DO";

export type PromptAction =
  | { kind: "navigate"; to: string }
  | { kind: "answer"; sessionId: string; key: "y" | "n" }
  | { kind: "kill"; sessionId: string }
  | { kind: "newSession" }
  | { kind: "toggleTheme" }
  | { kind: "signOut" };

export interface PromptSuggestion {
  id: string;
  group: PromptGroup;
  /** What the row shows in monospace, e.g. `go mac-mini/npm-install`. */
  command: string;
  detail: string;
  action: PromptAction;
  /** Tab replaces the input with this. */
  completion: string;
  /** ⌘↵ opens this Session in split view. */
  sessionId?: string;
}

export interface PromptIndex {
  sessions: Session[];
  machines: Machine[];
  recipes: Recipe[];
  settingsTabs: readonly SettingsTab[];
  /** Light or dark: the theme action says which it switches to. */
  theme: "light" | "dark";
  /** What the Machines found for `query` (5.S); ignored once the text moves on. */
  found?: { query: string; results: SearchResult[] };
}

const PER_GROUP = 5;
const TOTAL = 9;

/** Lower-case words joined by "-", as used in prompt paths. */
export function slug(text: string): string {
  return slugifyProject(text);
}

function machineOf(index: PromptIndex, session: Session): Machine | undefined {
  return index.machines.find((machine) => machine.id === session.machine);
}

/** `mac-mini/npm-install`: how the prompt names a Session. */
export function sessionPath(index: PromptIndex, session: Session): string {
  return `${slug(machineOf(index, session)?.name ?? session.machine)}/${slug(session.name)}`;
}

export { matchScore };

function ranked<T>(items: T[], query: string, keys: (item: T) => string[]): T[] {
  return items
    .map((item, order) => ({ item, order, score: matchScore(query, keys(item)) }))
    .filter((entry) => entry.score > 0)
    .sort((left, right) => right.score - left.score || left.order - right.order)
    .map((entry) => entry.item);
}

function sessionKeys(index: PromptIndex) {
  return (session: Session) => [sessionPath(index, session), session.name, session.command ?? "", session.project ?? ""];
}

function stateWord(session: Session): string {
  return session.state.charAt(0) + session.state.slice(1).toLowerCase();
}

// ── ANSWER ──────────────────────────────────────────────────────────────────

function answers(index: PromptIndex, key: "y" | "n", query: string): PromptSuggestion[] {
  const waiting = index.sessions
    .filter(canAnswer)
    .sort((left, right) => (right.updatedAt ?? 0) - (left.updatedAt ?? 0));
  return ranked(waiting, query, sessionKeys(index)).map((session) => {
    const path = sessionPath(index, session);
    return {
      id: `answer:${key}:${session.id}`,
      group: "ANSWER",
      command: `${key} → ${path}`,
      detail: [session.prompt, session.runningFor ? `waiting ${session.runningFor}` : undefined].filter(Boolean).join(" · "),
      action: { kind: "answer", sessionId: session.id, key },
      completion: `${key} ${path}`,
      sessionId: session.id,
    };
  });
}

// ── GO ──────────────────────────────────────────────────────────────────────

interface Place {
  name: string;
  keys: string[];
  to: string;
  detail: string;
}

function places(index: PromptIndex): Place[] {
  const pages: Place[] = [
    { name: "dashboard", keys: ["dashboard", "overview", "home"], to: "/app/overview", detail: "everything at a glance" },
    { name: "split", keys: ["split", "split view", "terminals"], to: "/app/split", detail: "terminals side by side" },
    { name: "history", keys: ["history", "commands"], to: "/app/history", detail: "every Command" },
    { name: "recipes", keys: ["recipes"], to: "/app/recipes", detail: "saved commands" },
    { name: "settings", keys: ["settings", "preferences"], to: "/app/settings", detail: "Machines, notifications, account" },
    ...index.settingsTabs.map((tab) => ({
      name: `settings/${tab.key}`,
      keys: [`settings ${tab.label}`, tab.label],
      to: `/app/settings/${tab.key}`,
      detail: `Settings · ${tab.label}`,
    })),
  ];
  const machines = index.machines.map((machine) => ({
    name: slug(machine.name),
    keys: [machine.name, slug(machine.name), "machine"],
    to: `/app/overview?machine=${encodeURIComponent(machine.id)}`,
    detail: `Machine · ${machine.status === "ONLINE" ? "online" : "offline"}`,
  }));
  const projects = Array.from(new Set(index.sessions.map((s) => s.project).filter((p): p is string => Boolean(p)))).map(
    (project) => ({
      name: `#${slug(project)}`,
      keys: [project, `#${project}`, "project"],
      to: `/app/projects/${slugifyProject(project)}`,
      detail: "project",
    }),
  );
  return [...pages, ...machines, ...projects];
}

function sessionGo(index: PromptIndex, session: Session): PromptSuggestion {
  const path = sessionPath(index, session);
  return {
    id: `go:${session.id}`,
    group: "GO",
    command: `go ${path}`,
    detail: [stateWord(session), session.result?.summary ?? session.prompt ?? session.runningFor].filter(Boolean).join(" · "),
    action: { kind: "navigate", to: `/app/sessions/${encodeURIComponent(session.id)}` },
    completion: `go ${path}`,
    sessionId: session.id,
  };
}

function placeGo(place: Place): PromptSuggestion {
  return {
    id: `go:${place.to}`,
    group: "GO",
    command: `go ${place.name}`,
    detail: place.detail,
    action: { kind: "navigate", to: place.to },
    completion: `go ${place.name}`,
  };
}

function go(index: PromptIndex, query: string): PromptSuggestion[] {
  if (!query) {
    const urgent = index.sessions.filter(needsAttention).map((session) => sessionGo(index, session));
    return [...urgent, ...places(index).slice(0, 5).map(placeGo)];
  }
  const sessions = ranked(index.sessions, query, sessionKeys(index)).map((session) => ({
    suggestion: sessionGo(index, session),
    score: matchScore(query, sessionKeys(index)(session)),
  }));
  const placeMatches = places(index)
    .map((place) => ({ suggestion: placeGo(place), score: matchScore(query, [place.name, ...place.keys]) }))
    .filter((entry) => entry.score > 0);
  return [...placeMatches, ...sessions].sort((left, right) => right.score - left.score).map((entry) => entry.suggestion);
}

// ── RUN ─────────────────────────────────────────────────────────────────────

/** `run "Qwen vLLM Server" on rtx-pc` → recipe text and machine text. */
export function parseRun(argument: string): { recipe: string; machine?: string } {
  const quoted = argument.match(/^"([^"]*)"?\s*(?:on\s+(\S+))?/i);
  if (quoted) return { recipe: quoted[1], machine: quoted[2] };
  const plain = argument.match(/^(.*?)(?:\s+on\s+(\S+))?$/i);
  return { recipe: (plain?.[1] ?? argument).trim(), machine: plain?.[2] };
}

function run(index: PromptIndex, argument: string): PromptSuggestion[] {
  const { recipe: recipeQuery, machine: machineQuery } = parseRun(argument);
  return ranked(index.recipes, recipeQuery, (recipe) => [recipe.name, recipe.id, recipe.category]).map((recipe) => {
    const allowed = recipe.machineIds?.length ? recipe.machineIds : [recipe.machine];
    const candidates = index.machines.filter((machine) => allowed.includes(machine.id));
    const chosen = machineQuery
      ? ranked(candidates, machineQuery, (machine) => [machine.name, slug(machine.name)])[0]
      : undefined;
    const target = chosen ?? candidates[0];
    const params = Object.keys(recipe.params).length;
    const to = `/app/recipes/${encodeURIComponent(recipe.id)}${chosen ? `?machine=${encodeURIComponent(chosen.id)}` : ""}`;
    return {
      id: `run:${recipe.id}`,
      group: "RUN",
      command: `run "${recipe.name}"${chosen ? ` on ${slug(chosen.name)}` : ""}`,
      detail: ["recipe", target?.name, `${params} parameter${params === 1 ? "" : "s"}`].filter(Boolean).join(" · "),
      action: { kind: "navigate", to },
      completion: `run "${recipe.name}"${chosen ? ` on ${slug(chosen.name)}` : ""}`,
    };
  });
}

// ── FIND ────────────────────────────────────────────────────────────────────

const KIND_WORD: Record<SearchResult["kind"], string> = {
  SESSIONS: "Session",
  COMMANDS: "Command",
  EVENTS: "Event",
  RECIPES: "Recipe",
  MACHINES: "Machine",
  PAGES: "Page",
};

/** What the Machines found for exactly this text, as Find rows. */
function foundRows(index: PromptIndex, query: string): PromptSuggestion[] {
  if (!index.found || index.found.query !== query) return [];
  return index.found.results.map((result) => ({
    id: `found:${result.id}`,
    group: "FIND",
    command: result.title,
    detail: [KIND_WORD[result.kind], index.machines.find((machine) => machine.id === result.machineId)?.name, result.detail]
      .filter(Boolean)
      .join(" · "),
    action: { kind: "navigate", to: result.to },
    // A Command that starts with the query completes to itself (5.A).
    completion:
      result.kind === "COMMANDS" && result.title.toLowerCase().startsWith(query.toLowerCase())
        ? `find ${result.title}`
        : `find ${query}`,
    sessionId: result.sessionId,
  }));
}

function find(index: PromptIndex, text: string): PromptSuggestion[] {
  const query = text.trim();
  return [
    ...foundRows(index, query),
    {
      id: `find:${query}`,
      group: "FIND",
      command: query ? `find ${query}` : "find …",
      detail: query ? "search Command history" : "type what to search history for",
      action: { kind: "navigate", to: query ? `/app/history?q=${encodeURIComponent(query)}` : "/app/history" },
      completion: `find ${query}`,
    },
  ];
}

// ── SPLIT ───────────────────────────────────────────────────────────────────

function split(index: PromptIndex, argument: string): PromptSuggestion[] {
  const words = argument.split(/\s+/).filter(Boolean);
  const resolved = words
    .map((word) => ranked(index.sessions, word, sessionKeys(index))[0])
    .filter((session): session is Session => Boolean(session));
  const unique = Array.from(new Map(resolved.map((session) => [session.id, session])).values()).slice(0, 4);
  const paths = unique.map((session) => sessionPath(index, session));
  const main: PromptSuggestion = {
    id: `split:${unique.map((s) => s.id).join(",")}`,
    group: "GO",
    command: paths.length ? `split ${paths.join(" ")}` : "split",
    detail: paths.length ? `${paths.length} terminal${paths.length === 1 ? "" : "s"} side by side` : "open split view",
    action: {
      kind: "navigate",
      to: unique.length ? `/app/split?sessions=${unique.map((s) => encodeURIComponent(s.id)).join(",")}` : "/app/split",
    },
    completion: `split ${paths.join(" ")} `.replace(/\s+$/, " "),
  };
  // Tab-completion candidates for the word being typed.
  const last = argument.endsWith(" ") ? "" : (words.at(-1) ?? "");
  const before = last ? words.slice(0, -1) : words;
  const candidates = last
    ? ranked(index.sessions, last, sessionKeys(index))
        .slice(0, 3)
        .map((session) => {
          const path = sessionPath(index, session);
          return {
            id: `split-add:${session.id}`,
            group: "GO" as const,
            command: `split … ${path}`,
            detail: `add ${session.name}`,
            action: main.action,
            completion: `split ${[...before, path].join(" ")} `,
            sessionId: session.id,
          };
        })
    : [];
  return [main, ...candidates];
}

// ── KILL ────────────────────────────────────────────────────────────────────

function kill(index: PromptIndex, query: string): PromptSuggestion[] {
  return ranked(index.sessions.filter(canInterrupt), query, sessionKeys(index)).map((session) => {
    const path = sessionPath(index, session);
    return {
      id: `kill:${session.id}`,
      group: "DO",
      command: `kill ${path}`,
      detail: `${stateWord(session)} · asks before sending Ctrl-C`,
      action: { kind: "kill", sessionId: session.id },
      completion: `kill ${path}`,
      sessionId: session.id,
    };
  });
}

// ── Bare text ───────────────────────────────────────────────────────────────

function actions(index: PromptIndex): Array<PromptSuggestion & { keys: string[] }> {
  const other = index.theme === "light" ? "dark" : "light";
  return [
    {
      id: "do:new",
      group: "DO",
      command: "new session",
      detail: "start a shell on a Machine",
      action: { kind: "newSession" },
      completion: "new session",
      keys: ["new session", "new terminal", "shell"],
    },
    {
      id: "do:theme",
      group: "DO",
      command: `${other} theme`,
      detail: `switch to the ${other} theme`,
      action: { kind: "toggleTheme" },
      completion: `${other} theme`,
      keys: ["theme", `${other} theme`, "appearance"],
    },
    {
      id: "do:signout",
      group: "DO",
      command: "sign out",
      detail: "end this browser session",
      action: { kind: "signOut" },
      completion: "sign out",
      keys: ["sign out", "log out", "logout"],
    },
  ];
}

function bare(index: PromptIndex, text: string): PromptSuggestion[] {
  const scored = actions(index).map(({ keys, ...suggestion }) => ({ suggestion, score: matchScore(text, keys) }));
  // Text that names an action ("new session", "theme", "sign out") leads with it.
  // Every typed word starts a word of the action's name (score ≥ 80).
  const named = scored.filter((entry) => entry.score >= 80).map((entry) => entry.suggestion);
  const loose = scored.filter((entry) => entry.score > 0 && entry.score < 80).map((entry) => entry.suggestion);
  return [...named, ...go(index, text), ...run(index, text), ...find(index, text), ...loose];
}

function empty(index: PromptIndex): PromptSuggestion[] {
  return [...answers(index, "y", ""), ...go(index, ""), ...find(index, "")];
}

// ── Entry point ─────────────────────────────────────────────────────────────

/**
 * Ghost text (5.A): the rest of `suggestion`'s completion after what is
 * typed, when it extends the input exactly; Tab accepts it.
 */
export function ghostText(input: string, suggestion: PromptSuggestion | undefined): string {
  if (!suggestion || !input || !suggestion.completion.startsWith(input)) return "";
  return suggestion.completion.slice(input.length);
}

/** Shortest text worth asking every Machine about. */
const MIN_SEARCH = 2;

/**
 * The text to search every Machine for: a `find` argument or bare text, never
 * another command (`go`, `run`, `y`, `split`, …), and not a single character.
 */
export function searchText(input: string): string | undefined {
  const text = input.trim();
  const head = text.split(/\s+/)[0]?.toLowerCase() ?? "";
  const hasArgument = /\s/.test(text);
  const argument = text.slice(head.length).trim();
  let query: string;
  if (head === "find") query = argument;
  else if ((head === "y" || head === "n") && (hasArgument || text.length === 1)) return undefined;
  else if (["go", "run", "kill"].includes(head) && hasArgument) return undefined;
  else if (head === "split") return undefined;
  else query = text;
  return query.length >= MIN_SEARCH ? query : undefined;
}

/** Suggestions for what has been typed, grouped in display order and capped. */
export function suggest(input: string, index: PromptIndex): PromptSuggestion[] {
  const text = input.replace(/^\s+/, "");
  const head = text.split(/\s+/)[0]?.toLowerCase() ?? "";
  const hasArgument = /\s/.test(text);
  const argument = text.slice(head.length).trim();
  let all: PromptSuggestion[];
  if (!text) all = empty(index);
  else if ((head === "y" || head === "n") && (hasArgument || text.length === 1)) all = answers(index, head, argument);
  else if (head === "go" && hasArgument) all = go(index, argument);
  else if (head === "run" && hasArgument) all = run(index, argument);
  else if (head === "find" && hasArgument) all = find(index, argument);
  else if (head === "split" && (hasArgument || text === "split")) all = split(index, text.slice(5).replace(/^\s+/, ""));
  else if (head === "kill" && hasArgument) all = kill(index, argument);
  else all = bare(index, text.trim());
  return capped(all);
}

/** Groups in the order their best suggestion arrived; at most 5 each and 9 in all. */
function capped(all: PromptSuggestion[]): PromptSuggestion[] {
  const seen = new Set<string>();
  const byGroup = new Map<PromptGroup, PromptSuggestion[]>();
  for (const suggestion of all) {
    if (seen.has(suggestion.id)) continue;
    seen.add(suggestion.id);
    const list = byGroup.get(suggestion.group) ?? [];
    if (list.length < PER_GROUP) list.push(suggestion);
    byGroup.set(suggestion.group, list);
  }
  return Array.from(byGroup.values()).flat().slice(0, TOTAL);
}
