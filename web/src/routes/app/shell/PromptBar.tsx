import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from "react";

import { useTheme } from "../../../context/ThemeContext";
import { dataAdapter } from "../../../data";
import { isMockData } from "../../../data/mode";
import { useLiveData } from "../../../data/LiveDataContext";
import { TermKeyboard } from "../../../terminal/keyboard/TermKeyboard";
import type { Recipe } from "../../../data/types";
import { settingsTabs } from "../settings-tabs";
import { suggest, type PromptGroup, type PromptSuggestion } from "./promptGrammar";
import styles from "./PromptBar.module.css";

const IS_MAC = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);

let recipeCache: Promise<Recipe[]> | null = null;
function loadRecipes(): Promise<Recipe[]> {
  recipeCache ??= dataAdapter.getRecipes().catch(() => {
    recipeCache = null;
    return [];
  });
  return recipeCache;
}

const GROUP_LABEL: Record<PromptGroup, string> = {
  ANSWER: "Answer",
  GO: "Go",
  RUN: "Run",
  FIND: "Find",
  DO: "Do",
};

/**
 * The prompt (GLASS_SPEC.md §5): bottom centre, deterministic grammar, the
 * page dimmed behind. It only proposes; the shell performs the chosen action
 * with the same confirmations the rest of the app uses.
 */
export function PromptBar({
  initialText,
  onClose,
  onChoose,
  sheet = false,
}: {
  initialText: string;
  onClose: () => void;
  onChoose: (suggestion: PromptSuggestion, options: { split: boolean }) => void;
  /** Phones (MOBILE_GLASS_SPEC §3): a bottom sheet typed on the built-in keyboard. */
  sheet?: boolean;
}) {
  const { machines, terminals } = useLiveData();
  const { theme } = useTheme();
  const [text, setText] = useState(initialText);
  const [selected, setSelected] = useState(0);
  const [recipes, setRecipes] = useState<Recipe[]>([]);
  const input = useRef<HTMLInputElement>(null);
  const listId = useId();

  useEffect(() => {
    let live = true;
    void loadRecipes().then((loaded) => live && setRecipes(loaded));
    return () => {
      live = false;
    };
  }, []);

  useEffect(() => {
    const field = input.current;
    if (!field) return;
    field.focus();
    field.setSelectionRange(field.value.length, field.value.length);
  }, []);

  const suggestions = useMemo(
    () =>
      suggest(text, {
        sessions: terminals,
        machines,
        recipes,
        settingsTabs: settingsTabs(isMockData),
        theme: theme === "light" ? "light" : "dark",
      }),
    [text, terminals, machines, recipes, theme],
  );
  const current = suggestions[Math.min(selected, suggestions.length - 1)];

  function choose(suggestion: PromptSuggestion | undefined, split = false) {
    if (!suggestion) return;
    onChoose(suggestion, { split });
  }

  /** Built-in keyboard bytes edit the input at its caret; ⏎ runs the selection. */
  function typed(bytes: string) {
    const field = input.current;
    const start = field?.selectionStart ?? text.length;
    const end = field?.selectionEnd ?? text.length;
    const place = (next: string, caret: number) => {
      setText(next);
      setSelected(0);
      requestAnimationFrame(() => field?.setSelectionRange(caret, caret));
    };
    if (bytes === "\r") choose(current);
    else if (bytes === "\x7f") {
      if (start !== end) place(text.slice(0, start) + text.slice(end), start);
      else if (start > 0) place(text.slice(0, start - 1) + text.slice(end), start - 1);
    } else if (bytes === "\x1b[D" || bytes === "\x1b[C") {
      const caret = Math.max(0, Math.min(text.length, start + (bytes === "\x1b[C" ? 1 : -1)));
      field?.setSelectionRange(caret, caret);
    } else if (!bytes.startsWith("\x1b") && !/[\x00-\x1f]/.test(bytes)) {
      place(text.slice(0, start) + bytes + text.slice(end), start + bytes.length);
    }
  }

  function onKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (suggestions.length === 0) return;
      const step = event.key === "ArrowDown" ? 1 : -1;
      setSelected((index) => (Math.min(index, suggestions.length - 1) + step + suggestions.length) % suggestions.length);
    } else if (event.key === "Tab") {
      if (!current) return;
      event.preventDefault();
      setText(current.completion);
      setSelected(0);
    } else if (event.key === "Enter") {
      event.preventDefault();
      choose(current, event.metaKey || event.ctrlKey);
    }
  }

  // Groups in suggestion order, so the selected first row is drawn first.
  const order = Array.from(new Set(suggestions.map((suggestion) => suggestion.group)));
  const groups = order.map((group) => ({
    group,
    items: suggestions.map((suggestion, index) => ({ suggestion, index })).filter(({ suggestion }) => suggestion.group === group),
  }));
  const optionId = (index: number) => `${listId}-option-${index}`;

  return (
    <div className={styles.layer} role="presentation" data-sheet={sheet || undefined}>
      <div className={styles.scrim} aria-hidden="true" onMouseDown={onClose} />
      <div className={styles.dock} role="dialog" aria-modal="true" aria-label="Prompt">
        {suggestions.length > 0 && (
          <div className={styles.panel}>
            <div id={listId} role="listbox" aria-label="Suggestions" className={styles.list}>
              {groups.map(({ group, items }) => (
                <div key={group} role="group" aria-labelledby={`${listId}-${group}`}>
                  <div id={`${listId}-${group}`} className={styles.groupLabel} role="presentation">
                    {GROUP_LABEL[group]}
                  </div>
                  {items.map(({ suggestion, index }) => {
                    const active = current === suggestion;
                    return (
                      <div
                        key={suggestion.id}
                        id={optionId(index)}
                        role="option"
                        aria-selected={active}
                        className={styles.row}
                        data-active={active || undefined}
                        // Movement, not enter: a list opening under a still cursor keeps its selection.
                        onMouseMove={() => selected !== index && setSelected(index)}
                        onMouseDown={(event) => {
                          event.preventDefault();
                          choose(suggestion, event.metaKey || event.ctrlKey);
                        }}
                      >
                        <span className={styles.command}>{suggestion.command}</span>
                        <span className={styles.detail}>{suggestion.detail}</span>
                        {active && (
                          <span className={styles.enter} aria-hidden="true">
                            ↵
                          </span>
                        )}
                      </div>
                    );
                  })}
                </div>
              ))}
            </div>
            {!sheet && (
            <div className={styles.footer} aria-hidden="true">
              <span>↵ run</span>
              <span>tab complete</span>
              <span>{IS_MAC ? "⌘↵" : "Ctrl ↵"} open in split</span>
              <span className={styles.footerEnd}>esc close</span>
            </div>
            )}
          </div>
        )}
        <label className={styles.inputPill}>
          <span className={styles.caret} aria-hidden="true">
            ❯
          </span>
          <input
            ref={input}
            className={styles.input}
            value={text}
            onChange={(event) => {
              setText(event.target.value);
              setSelected(0);
            }}
            onKeyDown={onKeyDown}
            role="combobox"
            aria-label="Command"
            aria-expanded={suggestions.length > 0}
            aria-controls={listId}
            aria-autocomplete="list"
            aria-activedescendant={current ? optionId(suggestions.indexOf(current)) : undefined}
            spellCheck={false}
            autoComplete="off"
            autoCapitalize="off"
            autoCorrect="off"
            inputMode={sheet ? "none" : undefined}
          />
          {sheet ? (
            <button type="button" className={styles.cancel} onClick={onClose}>
              Cancel
            </button>
          ) : (
            <span className={styles.grammar} aria-hidden="true">
              go · run · find · y · n · split · kill
            </span>
          )}
        </label>
        {sheet && <TermKeyboard onBytes={typed} accessory={false} />}
      </div>
    </div>
  );
}
