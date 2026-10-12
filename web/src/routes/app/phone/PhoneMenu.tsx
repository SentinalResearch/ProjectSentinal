import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { Link, useLocation } from "react-router-dom";

import { SentinalMark } from "../../../components/SentinalMark";
import { dataAdapter } from "../../../data";
import { useLiveData } from "../../../data/LiveDataContext";
import type { HistoryEntry, Recipe } from "../../../data/types";
import { isReadOnlyTerminalState } from "../../../terminal/lifecycle";
import { currentPage, PAGES, projectsOf, searchEverything } from "./menuSearch";
import { SearchIcon } from "./SearchIcon";
import styles from "./phoneMenu.module.css";

const FOCUSABLE = 'a[href], button:not([disabled]), input:not([disabled])';

/**
 * N1 Menu / N2 Search: a full-screen sheet opened from the page title or ⌕.
 * Search is focused on open; typing replaces the page list with grouped results.
 */
export function PhoneMenu({ needsYou, onClose }: { needsYou: number; onClose: (restoreFocus: boolean) => void }) {
  const { pathname } = useLocation();
  const { sessions, machines } = useLiveData();
  const [query, setQuery] = useState("");
  const [recipes, setRecipes] = useState<Recipe[]>([]);
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const dialogRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const active = currentPage(pathname);
  const open = sessions.filter((session) => !isReadOnlyTerminalState(session.state)).length;

  useEffect(() => {
    inputRef.current?.focus();
    let live = true;
    void dataAdapter.getRecipes().then((list) => live && setRecipes(list)).catch(() => undefined);
    return () => {
      live = false;
    };
  }, []);

  useEffect(() => {
    const trimmed = query.trim();
    if (!trimmed) {
      setHistory([]);
      return;
    }
    let live = true;
    const timer = window.setTimeout(() => {
      void dataAdapter.getHistory({ query: trimmed }).then((rows) => live && setHistory(rows)).catch(() => undefined);
    }, 150);
    return () => {
      live = false;
      window.clearTimeout(timer);
    };
  }, [query]);

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose(true);
      return;
    }
    if (event.key !== "Tab" || !dialogRef.current) return;
    const focusable = Array.from(dialogRef.current.querySelectorAll<HTMLElement>(FOCUSABLE));
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last?.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first?.focus();
    }
  }

  const meta: Record<string, string> = {
    Overview: needsYou > 0 ? `${needsYou} need you` : "",
    Terminals: `${open} open`,
    History: "",
    Recipes: recipes.length > 0 ? String(recipes.length) : "",
    Settings: "",
  };
  const groups = searchEverything(query, { sessions, machines, recipes, history });
  const projects = projectsOf(sessions);
  const leave = () => onClose(false);

  return (
    <div ref={dialogRef} className={styles.menu} role="dialog" aria-modal="true" aria-label="Menu" onKeyDown={onKeyDown}>
      <div className={styles.head}>
        <SentinalMark size={20} />
        <span className={styles.wordmark}>Sentinal</span>
        <button type="button" className={styles.close} aria-label="Close menu" onClick={() => onClose(true)}>
          ✕
        </button>
      </div>
      <div className={styles.body}>
        <label className={styles.search}>
          <span className={styles.searchGlyph}>
            <SearchIcon size={18} />
          </span>
          <input
            ref={inputRef}
            type="search"
            aria-label="Search"
            placeholder="Jump to page, session, machine…"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            autoComplete="off"
            spellCheck={false}
          />
        </label>

        {query.trim() ? (
          groups.length > 0 ? (
            groups.map((group) => (
              <section key={group.label} className={styles.group}>
                <h2 className={styles.label}>{group.label.toUpperCase()}</h2>
                <ul className={styles.results} aria-label={group.label}>
                  {group.results.map((result) => (
                    <li key={result.key}>
                      <Link to={result.to} className={styles.result} onClick={leave}>
                        <span className={styles.resultText}>
                          <span className={styles.resultTitle}>{result.title}</span>
                          <span className={styles.resultDetail}>{result.detail}</span>
                        </span>
                        <span className={styles.chevron} aria-hidden="true">›</span>
                      </Link>
                    </li>
                  ))}
                </ul>
              </section>
            ))
          ) : (
            <p className={styles.empty}>No matches for “{query.trim()}”.</p>
          )
        ) : (
          <>
            <nav className={styles.pages} aria-label="Pages">
              {PAGES.map((page) => {
                const current = page === active;
                return (
                  <Link
                    key={page.path}
                    to={page.path}
                    className={styles.page}
                    aria-current={current ? "page" : undefined}
                    onClick={leave}
                  >
                    <span className={styles.pageMark} aria-hidden="true" />
                    <span className={styles.pageLabel} data-page>{page.label}</span>
                    {meta[page.label] && <span className={styles.pageMeta}>{meta[page.label]}</span>}
                  </Link>
                );
              })}
            </nav>
            <section className={styles.group}>
              <h2 className={styles.label}>MACHINES</h2>
              <div className={styles.chips} role="group" aria-label="Machines">
                {machines.map((machine) => (
                  <Link
                    key={machine.id}
                    to={`/app/overview?machine=${encodeURIComponent(machine.id)}`}
                    className={styles.chip}
                    onClick={leave}
                  >
                    <span className={styles.dot} data-online={machine.status === "ONLINE"} aria-hidden="true" />
                    {machine.name}
                  </Link>
                ))}
              </div>
            </section>
            {projects.length > 0 && (
              <section className={styles.group}>
                <h2 className={styles.label}>PROJECTS</h2>
                <div className={styles.chips} role="group" aria-label="Projects">
                  {projects.map((project) => (
                    <Link key={project.slug} to={`/app/projects/${project.slug}`} className={styles.chip} onClick={leave}>
                      {project.name}
                    </Link>
                  ))}
                </div>
              </section>
            )}
          </>
        )}
      </div>
    </div>
  );
}
