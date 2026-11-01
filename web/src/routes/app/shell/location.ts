import { slugifyProject } from "../../../data/format";
import type { Machine, Session } from "../../../data/types";

/** What the island says about where you are. */
export type IslandLocation =
  | { kind: "page"; name: string }
  | { kind: "session"; machine: string; title: string; sessionId: string };

const PAGES: Array<[RegExp, string]> = [
  [/^\/app\/?$/, "Dashboard"],
  [/^\/app\/overview\/?$/, "Dashboard"],
  [/^\/app\/split\/?$/, "Split view"],
  [/^\/app\/history\/?$/, "History"],
  [/^\/app\/recipes(\/.*)?$/, "Recipes"],
  [/^\/app\/settings(\/.*)?$/, "Settings"],
];

/** Name the current page, or `machine › session` on a Session's pages. */
export function islandLocation(
  pathname: string,
  search: string,
  machines: Machine[],
  sessions: Session[],
): IslandLocation {
  const session = pathname.match(/^\/app\/sessions\/([^/]+)/);
  if (session) {
    const id = decodeURIComponent(session[1]);
    const found = sessions.find((entry) => entry.id === id);
    const machine = found ? machines.find((entry) => entry.id === found.machine) : undefined;
    return {
      kind: "session",
      sessionId: id,
      machine: machine?.name ?? found?.machine ?? "",
      title: found?.name ?? "Session",
    };
  }
  const machineId = new URLSearchParams(search).get("machine");
  if (/^\/app\/overview\/?$/.test(pathname) && machineId) {
    return { kind: "page", name: machines.find((entry) => entry.id === machineId)?.name ?? "Machine" };
  }
  const project = pathname.match(/^\/app\/projects\/([^/]+)/);
  if (project) {
    const slug = decodeURIComponent(project[1]);
    const name = sessions.find((entry) => entry.project && slugifyProject(entry.project) === slug)?.project;
    return { kind: "page", name: name ? `#${name}` : "Project" };
  }
  for (const [pattern, name] of PAGES) {
    if (pattern.test(pathname)) return { kind: "page", name };
  }
  return { kind: "page", name: "Sentinal" };
}

