import { useEffect, useRef } from "react";
import { useNavigate } from "react-router-dom";

import { CaptureBadge } from "../../components/CaptureBadge";
import { MockBadge } from "../../components/MockBadge";
import { StateTag } from "../../components/StateTag";
import { slugifyProject } from "../../data/format";
import { useLiveData } from "../../data/LiveDataContext";
import { useIsPhone } from "../../hooks/useViewport";
import styles from "./HistoryPage.module.css";
import { PhoneHistory } from "./phone/PhoneHistory";
import { DATE_BUCKETS, historyRowState, RESULT_LABEL, RESULTS, useHistoryQuery } from "./useHistoryQuery";

/** design_handoff_sentinal/design/SentinalHistory.dc.html */
export function HistoryPage() {
  return useIsPhone() ? <PhoneHistory /> : <DesktopHistory />;
}

function DesktopHistory() {
  const navigate = useNavigate();
  const { machines, sessions } = useLiveData();
  const inputRef = useRef<HTMLInputElement | null>(null);

  const {
    qInput,
    setQInput,
    rows,
    visible,
    machineFilter,
    projectSlug,
    resultFilter,
    dateFilter,
    projectOptions,
    setParam,
    reset,
  } = useHistoryQuery();

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

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
        {rows?.unavailableMachines && rows.unavailableMachines.length > 0 && (
          <p role="status" className={styles.subtitle}>
            Partial results · unavailable: {rows.unavailableMachines.map((id) => machines.find((m) => m.id === id)?.name ?? id).join(", ")}
          </p>
        )}
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
                <span className={`${styles.command} mono`}>
                  {entry.command}
                  {session && <CaptureBadge capture={session.capture ?? "FULL"} origin={session.origin} />}
                  {entry.ok === true && entry.machine && (
                    <button
                      type="button"
                      onClick={(event) => {
                        event.stopPropagation();
                        const query = new URLSearchParams({ machine: entry.machine!, command: entry.command });
                        if (entry.cwd) query.set("cwd", entry.cwd);
                        navigate(`/app/recipes/new?${query.toString()}`);
                      }}
                    >
                      Save as recipe
                    </button>
                  )}
                </span>
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
