/** One mounted page: its frame key outlives navigation, so its state does too. */
export interface PageFrame {
  key: string;
  pathname: string;
  search: string;
}

/** The page on top and the pages behind it, most recent first (GLASS_SPEC.md §6). */
export interface SheetStack {
  current: PageFrame;
  sheets: PageFrame[];
}

/** At most this many pages wait behind the current one. */
export const MAX_SHEETS = 3;

type Place = { pathname: string; search: string };

function samePlace(left: Place, right: Place): boolean {
  return left.pathname === right.pathname && left.search === right.search;
}

/**
 * The stack after a navigation. A replace (History updating `?q=` as you
 * type) changes the current frame in place; going to a page that is already
 * a sheet brings that same frame back; anything else pushes the current page
 * behind as the newest sheet and drops the oldest past three.
 */
export function nextStack(
  stack: SheetStack,
  place: Place,
  action: "PUSH" | "POP" | "REPLACE",
  newKey: () => string,
): SheetStack {
  if (action === "REPLACE" || samePlace(stack.current, place)) {
    return { ...stack, current: { ...stack.current, ...place } };
  }
  const index = stack.sheets.findIndex((sheet) => samePlace(sheet, place));
  const restored = index >= 0 ? stack.sheets[index] : undefined;
  const rest = stack.sheets.filter((_, at) => at !== index);
  return {
    current: restored ? { ...restored, ...place } : { key: newKey(), ...place },
    sheets: [stack.current, ...rest].slice(0, MAX_SHEETS),
  };
}
