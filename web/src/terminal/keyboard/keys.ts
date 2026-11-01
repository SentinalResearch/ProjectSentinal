/**
 * The built-in terminal keyboard's layouts and byte encoding
 * (sentinal-glass-handoff/specs/MOBILE_GLASS_SPEC.md §4). Pure: no DOM.
 */

/** Bytes for the named keys. */
export const SPECIAL = {
  enter: "\r",
  tab: "\t",
  esc: "\x1b",
  backspace: "\x7f",
  up: "\x1b[A",
  down: "\x1b[B",
  right: "\x1b[C",
  left: "\x1b[D",
} as const;

export type SpecialKey = keyof typeof SPECIAL;

/** A modifier's state: off, on for the next key, or on until tapped again. */
export type ModifierState = "off" | "latched" | "locked";

/** Two taps closer than this lock a modifier. */
export const DOUBLE_TAP_MS = 300;

/** Next modifier state after a tap at `now`, given the previous tap time. */
export function tapModifier(state: ModifierState, lastTap: number, now: number): ModifierState {
  if (state === "off") return "latched";
  if (state === "latched") return now - lastTap <= DOUBLE_TAP_MS ? "locked" : "off";
  return "off";
}

/** After a key is sent: a latch is used up, a lock stays. */
export function afterKey(state: ModifierState): ModifierState {
  return state === "latched" ? "off" : state;
}

export interface Modifiers {
  ctrl: boolean;
  alt: boolean;
}

/** Ctrl+X is the character's code & 0x1f; Alt prefixes Esc (§4 key codes). */
export function encodeChar(char: string, { ctrl, alt }: Modifiers): string {
  let out = char;
  if (ctrl && char.length === 1) out = String.fromCharCode(char.charCodeAt(0) & 0x1f);
  return alt ? `\x1b${out}` : out;
}

/** A named key with Alt as an Esc prefix; Ctrl does not change these. */
export function encodeSpecial(key: SpecialKey, { alt }: Modifiers): string {
  return alt ? `\x1b${SPECIAL[key]}` : SPECIAL[key];
}

/** Number row with the symbols ⇧ or a long press types. */
export const NUMBERS: ReadonlyArray<[string, string]> = [
  ["1", "!"], ["2", "@"], ["3", "#"], ["4", "$"], ["5", "%"],
  ["6", "^"], ["7", "&"], ["8", "*"], ["9", "("], ["0", ")"],
];

export const LETTER_ROWS: readonly string[][] = [
  ["q", "w", "e", "r", "t", "y", "u", "i", "o", "p"],
  ["a", "s", "d", "f", "g", "h", "j", "k", "l"],
  ["z", "x", "c", "v", "b", "n", "m"],
];

export const SYMBOL_ROWS: readonly string[][] = [
  ["-", "_", "=", "+", "[", "]", "{", "}", "\\", "|"],
  ["~", "`", ";", ":", "'", '"', ",", ".", "<", ">"],
];

/** Shell snippets that are awkward to type on glass; inserted as text, never with Enter. */
export const SNIPPETS: ReadonlyArray<{ label: string; text: string }> = [
  { label: "&&", text: "&&" },
  { label: "||", text: "||" },
  { label: "$(", text: "$(" },
  { label: "~/", text: "~/" },
  { label: "../", text: "../" },
  { label: "2>&1", text: "2>&1" },
  { label: "| grep", text: "| grep " },
];

export const SHIFTED_SYMBOLS: readonly string[] = ["!", "@", "#", "$", "%", "^", "&", "*", "?"];

/** The Ctrl layer names every combo, so nobody has to remember them. */
export const CTRL_COMBOS: ReadonlyArray<{ key: string; name: string; danger?: boolean }> = [
  { key: "c", name: "interrupt", danger: true },
  { key: "d", name: "EOF / exit" },
  { key: "z", name: "suspend" },
  { key: "l", name: "clear" },
  { key: "r", name: "search hist" },
  { key: "a", name: "line start" },
  { key: "e", name: "line end" },
  { key: "u", name: "delete line" },
  { key: "w", name: "delete word" },
  { key: "k", name: "kill to end" },
  { key: "p", name: "previous" },
  { key: "n", name: "next" },
];

/** Key repeat on hold: first repeat after this… */
export const REPEAT_DELAY_MS = 400;
/** …then one every this. */
export const REPEAT_EVERY_MS = 40;
/** Horizontal travel on the space bar per cursor step. */
export const SWIPE_STEP_PX = 14;
