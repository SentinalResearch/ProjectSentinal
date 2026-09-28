import { useEffect, useRef, useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";

import { MockBadge } from "../../components/MockBadge";
import { StateTag } from "../../components/StateTag";
import { dataAdapter } from "../../data";
import { slugifyProject } from "../../data/format";
import { useLiveData } from "../../data/LiveDataContext";
import type { HistoryEntry } from "../../data/types";
import type { SessionState } from "../../protocol/SessionState";
import styles from "./HistoryPage.module.css";

// QUESTION: these are the actual day-count buckets from SentinalHistory.dc.html's
// working prototype (dateMax), distinct from config.historyRelativeDates (a
// separate, flagged assumption used only for the STARTED column's display label).
// See QUESTIONS.md #16.
const DATE_BUCKETS = ["Today", "7 days", "30 days", "All time"] as const;
type DateBucket = (typeof DATE_BUCKETS)[number];
const DATE_MAX: Record<DateBucket, number> = { Today: 0, "7 days": 7, "30 days": 30, "All time": Number.POSITIVE_INFINITY };

const RESULTS = ["ok", "failed", "running"] as const;
const RESULT_LABEL: Record<(typeof RESULTS)[number], string> = { ok: "Success", failed: "Failed", running: "Running" };

function historyRowState(entry: HistoryEntry, sessionState: SessionState | undefined): SessionState {
  if (sessionState) return sessionState;
  if (entry.ok === false) return "FAILED";
  if (entry.ok === true) return "COMPLETED";
  return "RUNNING";
}

/** design_handoff_sentinal/design/SentinalHistory.dc.html */
export function HistoryPage() {
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();
  const { machines, sessions } = useLiveData();
  const inputRef = useRef<HTMLInputElement | null>(null);

  const [qInput, setQInput] = useState(searchParams.get("q") ?? "");
  const [rows, setRows] = useState<HistoryEntry[] | null>(null);

  const machineFilter = searchParams.get("machine") ?? "All";
  const projectSlug = searchParams.get("project") ?? "All";
  const resultFilter = (searchParams.get("result") ?? "All") as "All" | (typeof RESULTS)[number];
  const dateFilter = (searchParams.get("date") as DateBucket | null) ?? "All time";

  const projectOptions = Array.from(new Set(sessions.map((s) => s.project).filter((p): p is string => Boolean(p))));
  const projectName = projectSlug === "All" ? undefined : projectOptions.find((p) => slugifyProject(p) === projectSlug);

  // setQInput (plain useState) and setSearchParams (react-router) aren't guaranteed to land in
  // the same commit, so a debounced setTimeout closing over `searchParams` can fire against a
  // stale snapshot and stomp a later reset. Read through a ref (always current as of the latest
  // render) instead of the closed-over variable so setParam is never stale.
  const searchParamsRef = useRef(searchParams);
  searchParamsRef.current = searchParams;

  function setParam(key: string, value: string) {
    const next = new URLSearchParams(searchParamsRef.current);
    if (value === "All" || value === "") next.delete(key);
    else next.set(key, value);
    setSearchParams(next, { replace: true });
  }

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  // Debounce the free-text query so the mock adapter's simulated latency doesn't fire on every keystroke.
  useEffect(() => {
    const t = setTimeout(
      () => setParam("q", qInput.trim()),
      qInput === (searchParamsRef.current.get("q") ?? "") ? 0 : 250,
    );
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [qInput]);

  useEffect(() => {
    let cancelled = false;
    dataAdapter
      .getHistory({
        machine: machineFilter === "All" ? undefined : machineFilter,
        project: projectName,
        result: resultFilter === "All" ? undefined : resultFilter,
        query: searchParams.get("q") ?? undefined,
      })
      .then((entries) => {
        if (!cancelled) setRows(entries);
      });
    return () => {
      cancelled = true;
    };
  }, [machineFilter, projectName, resultFilter, searchParams]);

  const maxDays = DATE_MAX[dateFilter];
  const visible = (rows ?? []).filter((r) => (r.daysAgo ?? 0) <= maxDays);

  function reset() {
    setQInput("");
    setSearchParams({}, { replace: true });
  }

  function chipStyle(on: boolean) {
    return {
      background: on ? "var(--text-1)" : "transparent",
      color: on ? "var(--bg-0)" : "var(--text-2)",
      borderColor: on ? "var(--text-1)" : "var(--line-strong)",
    };
  }

  return (
    <div className={styles.page}>
      <header className={styles.header}>
        <h1 className={styles.title}>History</h1>
        <MockBadge />
        <span className={styles.subtitle}>Every command Sentinal has seen, on every machine.</span>
      </header>

      <div className={styles.searchBar}>
        <svg width="18" height="18" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" className={styles.searchIcon}>
          <circle cx="7" cy="7" r="4.5" />
          <path d="M10.5 10.5l3 3" />
        </svg>
        <input
          ref={inputRef}
          value={qInput}
          onChange={(e) => setQInput(e.target.value)}
          placeholder="Search commands, e.g. train.py"
          spellCheck={false}
          className={`${styles.searchInput} mono`}
        />
        {qInput && (
          <button type="button" className={styles.clearQ} onClick={() => setQInput("")}>
            Clear
          </button>
        )}
      </div>

      <div className={styles.filters}>
        <div className={styles.filterGroup}>
          <span className={styles.filterLabel}>MACHINE</span>
          <div className={styles.filterOptions}>
            <button type="button" style={chipStyle(machineFilter === "All")} className={styles.chip} onClick={() => setParam("machine", "All")}>
              All
            </button>
            {machines.map((m) => (
              <button
                key={m.id}
                type="button"
                style={chipStyle(machineFilter === m.id)}
                className={styles.chip}
                onClick={() => setParam("machine", m.id)}
              >
                {m.name}
              </button>
            ))}
          </div>
        </div>

        <div className={styles.filterGroup}>
          <span className={styles.filterLabel}>PROJECT</span>
          <div className={styles.filterOptions}>
            <button type="button" style={chipStyle(projectSlug === "All")} className={styles.chip} onClick={() => setParam("project", "All")}>
              All
            </button>
            {projectOptions.map((p) => (
              <button
                key={p}
                type="button"
                style={chipStyle(projectSlug === slugifyProject(p))}
                className={styles.chip}
                onClick={() => setParam("project", slugifyProject(p))}
              >
                {p}
              </button>
            ))}
          </div>
        </div>

        <div className={styles.filterGroup}>
          <span className={styles.filterLabel}>RESULT</span>
          <div className={styles.filterOptions}>
            <button type="button" style={chipStyle(resultFilter === "All")} className={styles.chip} onClick={() => setParam("result", "All")}>
              All
            </button>
            {RESULTS.map((r) => (
              <button key={r} type="button" style={chipStyle(resultFilter === r)} className={styles.chip} onClick={() => setParam("result", r)}>
                {RESULT_LABEL[r]}
              </button>
            ))}
          </div>
        </div>

        <div className={styles.filterGroup}>
          <span className={styles.filterLabel}>DATE</span>
          <div className={styles.filterOptions}>
            {DATE_BUCKETS.map((d) => (
              <button key={d} type="button" style={chipStyle(dateFilter === d)} className={styles.chip} onClick={() => setParam("date", d)}>
                {d}
              </button>
            ))}
          </div>
        </div>
      </div>

      <section className={styles.tableWrap}>
        <div className={styles.tableScroll}>
          <div className={styles.tableHead}>
            <span />
            <span>COMMAND · {visible.length} {visible.length === 1 ? "RESULT" : "RESULTS"}</span>
            <span>MACHINE</span>
            <span>CWD</span>
            <span>STARTED</span>
            <span className={styles.timeHead}>TIME</span>
          </div>

          {visible.map((entry) => {
            const session = entry.sessionId ? sessions.find((s) => s.id === entry.sessionId) : undefined;
            const machine = entry.machine ? machines.find((m) => m.id === entry.machine) : undefined;
            const state = historyRowState(entry, session?.state);
            const clickable = Boolean(session);
            const durationColor =
              entry.ok === false ? "var(--s-failed-fg)" : entry.ok === null ? `var(--s-${state.toLowerCase()}-fg)` : "var(--text-1)";
            return (
              <div
                key={entry.id}
                className={styles.row}
                style={{ cursor: clickable ? "pointer" : "default" }}
                onClick={clickable ? () => navigate(`/app/sessions/${entry.sessionId}`) : undefined}
              >
                <span className={styles.tagCell}>
                  <StateTag state={state} size="sm" iconOnly />
                </span>
                <span className={`${styles.command} mono`}>{entry.command}</span>
                <span className={styles.machineCell} style={{ color: machine?.status === "OFFLINE" ? "var(--text-3)" : "var(--text-2)" }}>
                  {machine?.name ?? "—"}
                </span>
                <span className={`${styles.projectCell} mono`}>{entry.cwd ?? entry.project ?? "—"}</span>
                <span className={styles.startedCell}>{entry.date ?? "—"}</span>
                <span className={styles.durationCell} style={{ color: durationColor }}>
                  {entry.duration}
                </span>
              </div>
            );
          })}

          {rows !== null && visible.length === 0 && (
            <div className={styles.empty}>
              <span className={styles.emptyTitle}>No commands match.</span>
              <button type="button" className={styles.resetBtn} onClick={reset}>
                Reset filters
              </button>
            </div>
          )}
        </div>
      </section>
    </div>
  );
}
