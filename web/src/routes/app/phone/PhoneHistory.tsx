import { useState } from "react";
import { useNavigate } from "react-router-dom";

import { BottomSheet } from "../../../components/BottomSheet";
import { StateTag } from "../../../components/StateTag";
import { slugifyProject } from "../../../data/format";
import { useLiveData } from "../../../data/LiveDataContext";
import type { HistoryEntry } from "../../../data/types";
import { DATE_BUCKETS, historyRowState, RESULT_LABEL, RESULTS, useHistoryQuery } from "../useHistoryQuery";
import shell from "./phone.module.css";
import styles from "./phoneLists.module.css";
import { PageTitle } from "./PageTitle";

/** D1 · History on phones, with the D2 filters sheet. */
export function PhoneHistory() {
  const navigate = useNavigate();
  const { machines, sessions } = useLiveData();
  const query = useHistoryQuery();
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [menuFor, setMenuFor] = useState<string | null>(null);
  const activeFilters = [query.machineFilter, query.projectSlug, query.resultFilter].filter((value) => value !== "All").length
    + (query.dateFilter === "All time" ? 0 : 1);

  function saveAsRecipe(entry: HistoryEntry) {
    const params = new URLSearchParams({ machine: entry.machine ?? "", command: entry.command });
    if (entry.cwd) params.set("cwd", entry.cwd);
    navigate(`/app/recipes/new?${params.toString()}`);
  }

  // Group by the relative date label (Today, Yesterday, …) the adapter provides.
  const groups: { label: string; entries: HistoryEntry[] }[] = [];
  for (const entry of query.visible) {
    const label = entry.date ?? "Earlier";
    const group = groups.find((candidate) => candidate.label === label);
    if (group) group.entries.push(entry);
    else groups.push({ label, entries: [entry] });
  }

  return (
    <div className={shell.page}>
      <PageTitle>History</PageTitle>
      <input
        type="search"
        aria-label="Search commands"
        className={`${styles.search} mono`}
        value={query.qInput}
        onChange={(event) => query.setQInput(event.target.value)}
        placeholder="Search commands"
        spellCheck={false}
      />
      <div className={styles.chipRow} role="group" aria-label="Quick filters">
        <button type="button" className={styles.chip} aria-pressed={activeFilters > 0} onClick={() => setFiltersOpen(true)}>
          Filters{activeFilters > 0 ? ` · ${activeFilters}` : ""}
        </button>
        {RESULTS.map((result) => (
          <button
            key={result}
            type="button"
            className={styles.chip}
            aria-pressed={query.resultFilter === result}
            onClick={() => query.setParam("result", query.resultFilter === result ? "All" : result)}
          >
            {RESULT_LABEL[result]}
          </button>
        ))}
      </div>
      {query.rows?.unavailableMachines && query.rows.unavailableMachines.length > 0 && (
        <p role="status" className={styles.subtitle}>
          Partial results · unavailable:{" "}
          {query.rows.unavailableMachines.map((id) => machines.find((machine) => machine.id === id)?.name ?? id).join(", ")}
        </p>
      )}
      {groups.map((group) => (
        <section key={group.label} aria-label={group.label}>
          <h2 className={styles.groupHead}>{group.label}</h2>
          <ul className={styles.list}>
            {group.entries.map((entry) => {
              const session = entry.sessionId ? sessions.find((candidate) => candidate.id === entry.sessionId) : undefined;
              const machine = machines.find((candidate) => candidate.id === entry.machine);
              const meta = [machine?.name ?? entry.machine, entry.project, entry.duration].filter(Boolean).join(" · ");
              return (
                <li key={entry.id} className={styles.historyRow}>
                  <span className={styles.stateSquare}>
                    <StateTag state={historyRowState(entry, session?.state)} size="sm" iconOnly />
                  </span>
                  <span className={styles.historyText}>
                    <span className={`${styles.historyCommand} mono`}>{entry.command}</span>
                    <span className={styles.historyMeta}>{meta}</span>
                  </span>
                  <button
                    type="button"
                    className={styles.moreButton}
                    aria-label={`More actions for ${entry.command}`}
                    aria-expanded={menuFor === entry.id}
                    onClick={() => setMenuFor(menuFor === entry.id ? null : entry.id)}
                  >
                    ⋯
                  </button>
                  {menuFor === entry.id && (
                    <div className={styles.menu} role="menu">
                      {entry.ok === true && entry.machine && (
                        <button type="button" role="menuitem" onClick={() => saveAsRecipe(entry)}>
                          Save as recipe
                        </button>
                      )}
                      <button
                        type="button"
                        role="menuitem"
                        disabled={!session}
                        onClick={() => session && navigate(`/app/sessions/${session.id}`)}
                      >
                        Open
                      </button>
                      <button
                        type="button"
                        role="menuitem"
                        onClick={() => {
                          void navigator.clipboard?.writeText(entry.command);
                          setMenuFor(null);
                        }}
                      >
                        Copy
                      </button>
                    </div>
                  )}
                </li>
              );
            })}
          </ul>
        </section>
      ))}
      {query.rows !== null && query.visible.length === 0 && (
        <div className={styles.emptyState}>
          <span>No commands match.</span>
          <button type="button" className={styles.chip} onClick={query.reset}>
            Reset filters
          </button>
        </div>
      )}

      <FiltersSheet open={filtersOpen} onClose={() => setFiltersOpen(false)} query={query} />
    </div>
  );
}

function FiltersSheet({ open, onClose, query }: { open: boolean; onClose: () => void; query: ReturnType<typeof useHistoryQuery> }) {
  const { machines } = useLiveData();
  const groups: { name: string; key: string; current: string; options: { value: string; label: string }[] }[] = [
    { name: "Machine", key: "machine", current: query.machineFilter, options: [{ value: "All", label: "All" }, ...machines.map((machine) => ({ value: machine.id, label: machine.name }))] },
    { name: "Project", key: "project", current: query.projectSlug, options: [{ value: "All", label: "All" }, ...query.projectOptions.map((project) => ({ value: slugifyProject(project), label: project }))] },
    { name: "Result", key: "result", current: query.resultFilter, options: [{ value: "All", label: "All" }, ...RESULTS.map((result) => ({ value: result, label: RESULT_LABEL[result] }))] },
    { name: "Date", key: "date", current: query.dateFilter, options: DATE_BUCKETS.map((bucket) => ({ value: bucket, label: bucket })) },
  ];
  const count = query.visible.length;
  return (
    <BottomSheet open={open} title="Filters" onClose={onClose}>
      {groups.map((group) => (
        <div key={group.key} role="group" aria-label={group.name} className={styles.sheetGroup}>
          <span className={styles.sheetLabel}>{group.name}</span>
          <div className={styles.wrapChips}>
            {group.options.map((option) => (
              <button
                key={option.value}
                type="button"
                className={styles.chip}
                aria-pressed={group.current === option.value}
                onClick={() => query.setParam(group.key, option.value === "All time" ? "All" : option.value)}
              >
                {option.label}
              </button>
            ))}
          </div>
        </div>
      ))}
      <div className={styles.sheetActions}>
        <button type="button" className={styles.outlineButton} onClick={query.reset}>
          Reset
        </button>
        <button type="button" className={styles.primaryButton} onClick={onClose}>
          Show {count} {count === 1 ? "result" : "results"}
        </button>
      </div>
    </BottomSheet>
  );
}
