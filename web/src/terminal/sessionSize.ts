/**
 * One size per Session (T3): the daemon owns it, the viewer the owner last
 * typed in or focused sets it, and every other viewer draws the same screen
 * at that size. This viewer then shrinks its font to fit the width (never
 * below MIN_FONT) and pans for the rest. Pure arithmetic.
 */

/** The terminal's normal font size, px. */
export const BASE_FONT = 13;
/** Smallest font a terminal shrinks to before it pans instead (owner decision 4). */
export const MIN_FONT = 9;

/** `SESSION_SIZE` as the terminal uses it. */
export interface SessionSize {
  cols: number;
  rows: number;
  /** This viewer's report set the size. */
  owner: boolean;
}

/** Cells a box holds at the base font, from what fits at `font`. */
export function baseCells(cols: number, rows: number, font: number): { cols: number; rows: number } {
  const scale = font / BASE_FONT;
  return { cols: Math.max(1, Math.floor(cols * scale)), rows: Math.max(1, Math.floor(rows * scale)) };
}

/**
 * Font size that fits `sessionCols` into a box of `boxCols` (counted at the
 * base font): the base font when it fits, else smaller in half-pixel steps,
 * never below MIN_FONT (then the view pans).
 */
export function fitFont(sessionCols: number, boxCols: number): number {
  if (sessionCols <= boxCols) return BASE_FONT;
  // One column spare: cell widths are fractional pixels at a scaled font.
  const font = Math.floor(((BASE_FONT * Math.max(1, boxCols - 1)) / sessionCols) * 2) / 2;
  return Math.max(MIN_FONT, font);
}

/** The screen is sized for another viewer and does not fit this box as it is. */
export function sizedForOther(size: SessionSize | null, box: { cols: number; rows: number } | null): boolean {
  if (!size || size.owner || !box) return false;
  return size.cols !== box.cols || size.rows !== box.rows;
}
