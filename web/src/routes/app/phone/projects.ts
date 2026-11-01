import { slugifyProject } from "../../../data/format";
import type { Session } from "../../../data/types";

/** The distinct projects named by these Sessions, with their URL slugs. */
export function projectsOf(sessions: Session[]): { name: string; slug: string }[] {
  const names = Array.from(new Set(sessions.map((session) => session.project).filter((name): name is string => Boolean(name))));
  return names.map((name) => ({ name, slug: slugifyProject(name) }));
}
