import { useEffect, useRef, useState } from "react";
import { useSearchParams } from "react-router-dom";

import { dataAdapter } from "../../data";
import { slugifyProject } from "../../data/format";
import { useLiveData } from "../../data/LiveDataContext";
import type { HistoryEntry, HistoryResults } from "../../data/types";
import type { SessionState } from "../../protocol/SessionState";

// QUESTION: these are the actual day-count buckets from SentinalHistory.dc.html's
// working prototype (dateMax), distinct from config.historyRelativeDates (a
// separate, flagged assumption used only for the STARTED column's display label).
// See QUESTIONS.md #16.
export const DATE_BUCKETS = ["Today", "7 days", "30 days", "All time"] as const;
export type DateBucket = (typeof DATE_BUCKETS)[number];
const DATE_MAX: Record<DateBucket, number> = { Today: 0, "7 days": 7, "30 days": 30, "All time": Number.POSITIVE_INFINITY };

export const RESULTS = ["ok", "failed", "running"] as const;
export const RESULT_LABEL: Record<(typeof RESULTS)[number], string> = { ok: "Success", failed: "Failed", running: "Running" };

export function historyRowState(entry: HistoryEntry, sessionState: SessionState | undefined): SessionState {
  if (sessionState) return sessionState;
  if (entry.ok === false) return "FAILED";
  if (entry.ok === true) return "COMPLETED";
  return "RUNNING";
}

/** History search and filters, kept in the URL; shared by the desktop and phone layouts. */
export function useHistoryQuery() {
  const [searchParams, setSearchParams] = useSearchParams();
  const { sessions } = useLiveData();
  const [qInput, setQInput] = useState(searchParams.get("q") ?? "");
  const [rows, setRows] = useState<HistoryResults | null>(null);

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

  return {
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
  };
}
