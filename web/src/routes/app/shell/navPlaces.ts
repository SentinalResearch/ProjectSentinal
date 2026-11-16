/** The main places in the app, shown by the island's page-name menu. */
export interface NavPlace {
  label: string;
  to: string;
  /** What to type in the prompt to get there: clicking teaches the fast path. */
  word: string;
}

export const NAV_PLACES: NavPlace[] = [
  { label: "Overview", to: "/app/overview", word: "dashboard" },
  { label: "Terminals", to: "/app/split", word: "split" },
  { label: "Machines", to: "/app/settings/machines", word: "machines" },
  { label: "History", to: "/app/history", word: "history" },
  { label: "Recipes", to: "/app/recipes", word: "recipes" },
  { label: "Settings", to: "/app/settings", word: "settings" },
];

/** The one place a path belongs to; a Session or Project belongs to Overview, where it is listed. */
export function activePlace(pathname: string): NavPlace | undefined {
  const path = pathname.replace(/\/+$/, "");
  if (
    path === "/app" ||
    path.startsWith("/app/overview") ||
    path.startsWith("/app/sessions") ||
    path.startsWith("/app/projects")
  ) {
    return NAV_PLACES[0];
  }
  const inPlace = NAV_PLACES.filter((place) => path === place.to || path.startsWith(`${place.to}/`));
  return inPlace.sort((left, right) => right.to.length - left.to.length)[0];
}
