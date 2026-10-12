import { slugifyProject } from "../../../data/format";
import type { HistoryEntry, Machine, Recipe, Session } from "../../../data/types";

/** The five top-level phone pages, in menu order (MOBILE_SPEC.md → Navigation). */
export const PAGES = [
  { label: "Overview", path: "/app/overview", owns: ["/app", "/app/overview"] },
  { label: "Terminals", path: "/app/split", owns: ["/app/split"] },
  { label: "History", path: "/app/history", owns: ["/app/history"] },
  { label: "Recipes", path: "/app/recipes", owns: ["/app/recipes"] },
  { label: "Settings", path: "/app/settings", owns: ["/app/settings", "/app/more"] },
] as const;

/** Paths that show the top bar; everything else is a detail screen with a back bar. */
export function isTopLevel(pathname: string): boolean {
  return PAGES.some((page) => page.owns.some((owned) => pathname === owned));
}

export function currentPage(pathname: string): (typeof PAGES)[number] | undefined {
  return PAGES.find((page) =>
    page.owns.some((owned) => pathname === owned || (owned !== "/app" && pathname.startsWith(`${owned}/`))),
  );
}

export interface SearchResult {
  key: string;
  title: string;
  detail: string;
  to: string;
}

export interface SearchGroup {
  label: "Pages" | "Sessions" | "Machines" | "Recipes" | "History";
  results: SearchResult[];
}

const PER_GROUP = 5;

function matches(query: string, ...fields: (string | null | undefined)[]): boolean {
  return fields.some((field) => field?.toLowerCase().includes(query));
}

/** Filter pages, Sessions, Machines, Recipes, and History by one query, grouped by type. */
export function searchEverything(
  rawQuery: string,
  data: { sessions: Session[]; machines: Machine[]; recipes: Recipe[]; history: HistoryEntry[] },
): SearchGroup[] {
  const query = rawQuery.trim().toLowerCase();
  if (!query) return [];
  const machineName = (id: string | undefined) => data.machines.find((machine) => machine.id === id)?.name ?? id ?? "";
  const groups: SearchGroup[] = [
    {
      label: "Pages",
      results: PAGES.filter((page) => matches(query, page.label)).map((page) => ({
        key: page.path, title: page.label, detail: "Page", to: page.path,
      })),
    },
    {
      label: "Sessions",
      results: data.sessions
        .filter((session) => matches(query, session.name, session.command, session.project, machineName(session.machine)))
        .map((session) => ({
          key: session.id,
          title: `${session.name} · ${session.state}`,
          detail: [machineName(session.machine), session.command].filter(Boolean).join(" · "),
          to: `/app/sessions/${session.id}`,
        })),
    },
    {
      label: "Machines",
      results: data.machines.filter((machine) => matches(query, machine.name)).map((machine) => ({
        key: machine.id,
        title: machine.name,
        detail: machine.status === "ONLINE" ? "online" : machine.status.toLowerCase(),
        to: `/app/overview?machine=${encodeURIComponent(machine.id)}`,
      })),
    },
    {
      label: "Recipes",
      results: data.recipes.filter((recipe) => matches(query, recipe.name, recipe.template, recipe.category)).map((recipe) => ({
        key: recipe.id,
        title: recipe.name,
        detail: recipe.template,
        to: `/app/recipes/${encodeURIComponent(recipe.id)}`,
      })),
    },
    {
      label: "History",
      results: data.history.filter((entry) => matches(query, entry.command)).map((entry) => ({
        key: entry.id,
        title: entry.command,
        detail: [machineName(entry.machine), entry.date, entry.duration].filter(Boolean).join(" · "),
        to: `/app/history?${new URLSearchParams({ q: entry.command })}`,
      })),
    },
  ];
  return groups
    .map((group) => ({ ...group, results: group.results.slice(0, PER_GROUP) }))
    .filter((group) => group.results.length > 0);
}

export function projectsOf(sessions: Session[]): { name: string; slug: string }[] {
  const names = Array.from(new Set(sessions.map((session) => session.project).filter((name): name is string => Boolean(name))));
  return names.map((name) => ({ name, slug: slugifyProject(name) }));
}
