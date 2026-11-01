import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";

import {
  afterKey,
  CTRL_COMBOS,
  encodeChar,
  encodeSpecial,
  LETTER_ROWS,
  NUMBERS,
  REPEAT_DELAY_MS,
  REPEAT_EVERY_MS,
  SHIFTED_SYMBOLS,
  SNIPPETS,
  SWIPE_STEP_PX,
  SYMBOL_ROWS,
  tapModifier,
  type ModifierState,
  type SpecialKey,
} from "./keys";
import styles from "./TermKeyboard.module.css";

/** A suggestion chip: tapping inserts its text, never Enter. */
export interface KeyboardChip {
  label: string;
  text: string;
  /** Completions are tinted with the accent. */
  accent?: boolean;
}

export interface TermKeyboardProps {
  /** Every key press as the bytes a terminal expects. */
  onBytes: (bytes: string) => void;
  /** Chips on the letters layer (y / n, completions for the line, clear, exit). */
  chips?: KeyboardChip[];
  /** Chips on the symbols layer (tokens from recent output). */
  symbolChips?: KeyboardChip[];
  /** The Ctrl · Alt · Esc · Tab · arrows row; the prompt sheet leaves it out. */
  accessory?: boolean;
  /** With a hardware keyboard: only the accessory row is shown. */
  accessoryOnly?: boolean;
}

type Layer = "letters" | "symbols";

function haptic() {
  if (typeof navigator !== "undefined" && "vibrate" in navigator) navigator.vibrate?.(5);
}

/**
 * The built-in terminal keyboard (MOBILE_GLASS_SPEC §4): replaces the system
 * keyboard in the terminal and the prompt. Pure UI: it only emits bytes.
 */
export function TermKeyboard({ onBytes, chips = [], symbolChips = [], accessory = true, accessoryOnly = false }: TermKeyboardProps) {
  const [layer, setLayer] = useState<Layer>("letters");
  const [shift, setShift] = useState<ModifierState>("off");
  const [ctrl, setCtrl] = useState<ModifierState>("off");
  const [alt, setAlt] = useState<ModifierState>("off");
  // With Ctrl on, the Ctrl grid shows unless the owner chose letters ("abc").
  const [ctrlLetters, setCtrlLetters] = useState(false);
  const taps = useRef({ shift: 0, ctrl: 0, alt: 0 });
  const mods = useRef({ shift, ctrl, alt });
  mods.current = { shift, ctrl, alt };
  const emit = useRef(onBytes);
  emit.current = onBytes;

  /** Send one character with the current modifiers, then use up any latches. */
  const sendChar = (char: string) => {
    const { ctrl: c, alt: a } = mods.current;
    emit.current(encodeChar(char, { ctrl: c !== "off", alt: a !== "off" }));
    release();
  };
  const sendSpecial = (key: SpecialKey) => {
    const { ctrl: c, alt: a } = mods.current;
    emit.current(encodeSpecial(key, { ctrl: c !== "off", alt: a !== "off" }));
    release();
  };
  const sendText = (text: string) => emit.current(text);
  function release() {
    const { shift: s, ctrl: c, alt: a } = mods.current;
    if (s === "latched") setShift(afterKey(s));
    if (c === "latched") {
      setCtrl("off");
      setCtrlLetters(false);
    }
    if (a === "latched") setAlt(afterKey(a));
  }

  function tap(which: "shift" | "ctrl" | "alt") {
    const now = Date.now();
    const set = which === "shift" ? setShift : which === "ctrl" ? setCtrl : setAlt;
    const next = tapModifier(mods.current[which], taps.current[which], now);
    taps.current[which] = now;
    set(next);
    if (which === "ctrl" && next === "off") setCtrlLetters(false);
  }

  const shifted = shift !== "off";
  const ctrlGrid = ctrl !== "off" && !ctrlLetters;

  const accessoryRow = (
    <div className={styles.row} role="group" aria-label="Terminal keys">
      <ModKey label="Ctrl" state={ctrl} onTap={() => tap("ctrl")} />
      <ModKey label="Alt" state={alt} onTap={() => tap("alt")} />
      <Key label="Esc" onPress={() => sendSpecial("esc")} className={styles.mod} />
      <Key label="Tab" onPress={() => sendSpecial("tab")} className={styles.mod} />
      <Key label="←" name="Left" repeat onPress={() => sendSpecial("left")} />
      <Key label="↑" name="Up" repeat onPress={() => sendSpecial("up")} />
      <Key label="↓" name="Down" repeat onPress={() => sendSpecial("down")} />
      <Key label="→" name="Right" repeat onPress={() => sendSpecial("right")} />
    </div>
  );

  if (accessoryOnly) {
    return (
      <div className={styles.panel} data-compact role="group" aria-label="Terminal keyboard">
        {accessoryRow}
      </div>
    );
  }

  const strip = layer === "symbols" ? symbolChips : chips;

  return (
    <div className={styles.panel} role="group" aria-label="Terminal keyboard">
      {ctrl !== "off" && accessory && (
        <p className={styles.notice} role="status">
          Ctrl is on: next key is sent with Ctrl
        </p>
      )}
      {strip.length > 0 && (
        <div className={styles.chips} role="group" aria-label="Suggestions">
          {strip.map((chip) => (
            <button
              key={chip.label}
              type="button"
              className={styles.chip}
              data-accent={chip.accent || undefined}
              onPointerDown={preventFocus}
              onClick={() => {
                haptic();
                sendText(chip.text);
              }}
            >
              {chip.label}
            </button>
          ))}
        </div>
      )}
      {accessory && accessoryRow}

      {ctrlGrid ? (
        <>
          <div className={styles.ctrlGrid} role="group" aria-label="Ctrl combinations">
            {CTRL_COMBOS.map((combo) => (
              <Key
                key={combo.key}
                label={`^${combo.key.toUpperCase()}`}
                sub={combo.name}
                name={`Control ${combo.key.toUpperCase()}, ${combo.name}`}
                className={combo.danger ? `${styles.ctrlKey} ${styles.danger}` : styles.ctrlKey}
                onPress={() => sendChar(combo.key)}
              />
            ))}
          </div>
          <p className={styles.hint}>or tap any letter for Ctrl + letter</p>
        </>
      ) : layer === "letters" ? (
        <>
          <div className={styles.row} data-numbers>
            {NUMBERS.map(([digit, symbol]) => (
              <Key
                key={digit}
                label={shifted ? symbol : digit}
                sub={shifted ? undefined : symbol}
                onPress={() => sendChar(shifted ? symbol : digit)}
                onLongPress={() => sendChar(symbol)}
              />
            ))}
          </div>
          {LETTER_ROWS.slice(0, 2).map((row, index) => (
            <div key={index} className={styles.row} data-inset={index === 1 || undefined}>
              {row.map((letter) => (
                <Key key={letter} label={shifted ? letter.toUpperCase() : letter} onPress={() => sendChar(shifted ? letter.toUpperCase() : letter)} />
              ))}
            </div>
          ))}
          <div className={styles.row}>
            <ModKey label="⇧" name="Shift" state={shift} onTap={() => tap("shift")} wide />
            {LETTER_ROWS[2].map((letter) => (
              <Key key={letter} label={shifted ? letter.toUpperCase() : letter} onPress={() => sendChar(shifted ? letter.toUpperCase() : letter)} />
            ))}
            <Key label="⌫" name="Delete" sub="hold" repeat className={`${styles.mod} ${styles.wide}`} onPress={() => sendSpecial("backspace")} />
          </div>
        </>
      ) : (
        <>
          {SYMBOL_ROWS.map((row, index) => (
            <div key={index} className={styles.row}>
              {row.map((symbol) => (
                <Key key={symbol} label={symbol} onPress={() => sendChar(symbol)} />
              ))}
            </div>
          ))}
          <div className={styles.row} role="group" aria-label="Snippets">
            {SNIPPETS.map((snippet) => (
              <Key key={snippet.label} label={snippet.label} className={styles.snippet} onPress={() => sendText(snippet.text)} />
            ))}
            <Key label="⌫" name="Delete" repeat className={styles.mod} onPress={() => sendSpecial("backspace")} />
          </div>
          <div className={styles.row}>
            {SHIFTED_SYMBOLS.map((symbol) => (
              <Key key={symbol} label={symbol} onPress={() => sendChar(symbol)} />
            ))}
          </div>
        </>
      )}

      <div className={styles.row}>
        <Key
          label={ctrlGrid || layer === "symbols" ? "abc" : "#+="}
          name={ctrlGrid || layer === "symbols" ? "Letters" : "Symbols"}
          className={`${styles.mod} ${styles.wide}`}
          onPress={() => {
            if (ctrlGrid) setCtrlLetters(true);
            else setLayer(layer === "symbols" ? "letters" : "symbols");
          }}
        />
        <Key label="|" onPress={() => sendChar("|")} />
        <Key label="~" onPress={() => sendChar("~")} />
        <Key label="/" onPress={() => sendChar("/")} />
        <SpaceKey onSpace={() => sendChar(" ")} onMove={(key) => sendSpecial(key)} />
        <Key label="-" onPress={() => sendChar("-")} />
        <Key label="⏎" name="Enter" className={`${styles.enter} ${styles.wide}`} onPress={() => sendSpecial("enter")} />
      </div>
    </div>
  );
}

/** Keep focus where it is (the terminal) when a key is touched. */
function preventFocus(event: ReactPointerEvent) {
  event.preventDefault();
}

function Key({
  label,
  name,
  sub,
  className,
  repeat = false,
  onPress,
  onLongPress,
}: {
  label: ReactNode;
  name?: string;
  sub?: string;
  className?: string;
  /** Hold to repeat (⌫, arrows). */
  repeat?: boolean;
  onPress: () => void;
  /** Hold to type an alternative (the number row's symbols). */
  onLongPress?: () => void;
}) {
  const timer = useRef<number | undefined>(undefined);
  const handled = useRef(false);
  const stop = () => {
    window.clearTimeout(timer.current);
    window.clearInterval(timer.current);
    timer.current = undefined;
  };
  useEffect(() => stop, []);

  return (
    <button
      type="button"
      className={`${styles.key} ${className ?? ""}`}
      aria-label={name}
      onPointerDown={(event) => {
        preventFocus(event);
        haptic();
        handled.current = false;
        if (repeat) {
          handled.current = true;
          onPress();
          timer.current = window.setTimeout(() => {
            timer.current = window.setInterval(onPress, REPEAT_EVERY_MS);
          }, REPEAT_DELAY_MS);
        } else if (onLongPress) {
          timer.current = window.setTimeout(() => {
            handled.current = true;
            onLongPress();
          }, REPEAT_DELAY_MS);
        }
      }}
      onPointerUp={stop}
      onPointerCancel={stop}
      onPointerLeave={stop}
      onClick={() => {
        // A click after a pointer press that already acted (repeat or long press) does nothing.
        if (handled.current) {
          handled.current = false;
          return;
        }
        onPress();
      }}
    >
      <span className={styles.label}>{label}</span>
      {sub && <span className={styles.sub}>{sub}</span>}
    </button>
  );
}

function ModKey({
  label,
  name,
  state,
  onTap,
  wide = false,
}: {
  label: string;
  name?: string;
  state: ModifierState;
  onTap: () => void;
  wide?: boolean;
}) {
  return (
    <button
      type="button"
      className={`${styles.key} ${styles.mod} ${wide ? styles.wide : ""}`}
      data-state={state}
      aria-label={name ?? label}
      aria-pressed={state !== "off"}
      onPointerDown={(event) => {
        preventFocus(event);
        haptic();
      }}
      onClick={onTap}
    >
      <span className={styles.label}>{label}</span>
      {state !== "off" && <span className={styles.sub}>{state}</span>}
    </button>
  );
}

/** The space bar; a horizontal swipe on it moves the cursor instead (§4). */
function SpaceKey({ onSpace, onMove }: { onSpace: () => void; onMove: (key: "left" | "right") => void }) {
  const start = useRef<number | null>(null);
  const moved = useRef(false);
  return (
    <button
      type="button"
      className={`${styles.key} ${styles.space}`}
      aria-label="Space"
      onPointerDown={(event) => {
        preventFocus(event);
        haptic();
        start.current = event.clientX;
        moved.current = false;
        event.currentTarget.setPointerCapture?.(event.pointerId);
      }}
      onPointerMove={(event) => {
        if (start.current === null) return;
        let travel = event.clientX - start.current;
        while (Math.abs(travel) >= SWIPE_STEP_PX) {
          onMove(travel > 0 ? "right" : "left");
          moved.current = true;
          start.current += travel > 0 ? SWIPE_STEP_PX : -SWIPE_STEP_PX;
          travel = event.clientX - start.current;
        }
      }}
      onPointerUp={() => {
        start.current = null;
      }}
      onPointerCancel={() => {
        start.current = null;
      }}
      onClick={() => {
        if (moved.current) {
          moved.current = false;
          return;
        }
        onSpace();
      }}
    >
      <span className={styles.label}>space</span>
      <span className={styles.sub}>swipe = cursor</span>
    </button>
  );
}
