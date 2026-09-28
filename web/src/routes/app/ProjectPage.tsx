import { useEffect, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";

import { MockBadge } from "../../components/MockBadge";
import { StateTag } from "../../components/StateTag";
import { exitUnknown } from "../../data/capture";
import { dataAdapter } from "../../data";
import { formatSessionDuration, formatSessionFact, slugifyProject } from "../../data/format";
import { useLiveData } from "../../data/LiveDataContext";
import type { HistoryEntry } from "../../data/types";
import { EventsList } from "./EventsList";
import styles from "./ProjectPage.module.css";

/** design_handoff_sentinal/design/SentinalProject.dc.html */
export function ProjectPage() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { machines, sessions, loading } = useLiveData();
  const [history, setHistory] = useState<HistoryEntry[]>([]);

  const projectSessions = sessions.filter((s) => s.project && slugifyProject(s.project) === id);
  const projectName = projectSessions[0]?.project;

  useEffect(() => {
    if (!projectName) return;
    let cancelled = false;
    dataAdapter.getHistory({ project: projectName }).then((entries) => {
      if (!cancelled) setHistory(entries);
    });
    return () => {
      cancelled = true;
    };
  }, [projectName]);

  if (loading) return <div className={styles.page}>Loading…</div>;

  if (!projectName) {
    return (
      <div className={styles.notFound}>
        <p>Project not found.</p>
        <button type="button" className={styles.link} onClick={() => navigate("/app/overview")}>
          Back to Overview
        </button>
      </div>
    );
  }

  const path = projectSessions.find((s) => s.cwd)?.cwd;
  const machineNames = Array.from(
    new Set(projectSessions.map((s) => machines.find((m) => m.id === s.machine)?.name).filter((n): n is string => Boolean(n))),
  );
  const summary = `${projectSessions.length} session${projectSessions.length === 1 ? "" : "s"}${
    machineNames.length > 0 ? ` · ${machineNames.join(", ")}` : ""
  }`;

  const journalEvents = projectSessions.flatMap((s) => (s.events ?? []).map((event) => ({ event, sessionName: s.name })));

  return (
    <div className={styles.page}>
      <header className={styles.header}>
        <div className={styles.overlineRow}>
          <span className={styles.overline}>PROJECT</span>
          <MockBadge />
        </div>
        <h1 className={styles.title}>{projectName}</h1>
        <div className={styles.metaRow}>
          {path && <span className={`${styles.path} mono`}>{path}</span>}
          <span className={styles.summary}>{summary}</span>
        </div>
      </header>

      <div className={styles.body}>
        <div className={styles.main}>
          <section className={styles.section}>
            <div className={styles.sectionHead}>
              <span className={styles.sectionTitle}>Sessions</span>
              <span className={styles.sectionHint}>across machines</span>
            </div>
            {projectSessions.map((s) => {
              const machine = machines.find((m) => m.id === s.machine);
              return (
                <button key={s.id} type="button" className={styles.sessionRow} onClick={() => navigate(`/app/sessions/${s.id}`)}>
                  <StateTag state={s.state} size="sm" exitUnknown={exitUnknown(s)} />
                  <span className={styles.sessionMain}>
                    <span className={styles.sessionName}>{s.name}</span>
                    {s.command && <span className={`${styles.sessionCommand} mono`}>{s.command}</span>}
                  </span>
                  <span className={styles.sessionAside}>
                    <span className={styles.sessionFact}>{formatSessionFact(s)}</span>
                    <span className={styles.sessionMachine}>{machine?.name ?? s.machine}</span>
                  </span>
                </button>
              );
            })}
          </section>

          <section className={styles.section}>
            <div className={styles.sectionHead}>
              <span className={styles.sectionTitle}>Command history</span>
              <button
                type="button"
                className={styles.searchAll}
                onClick={() => navigate(`/app/history?project=${slugifyProject(projectName)}`)}
              >
                Search all →
              </button>
            </div>
            {history.length === 0 ? (
              <span className={styles.emptyHint}>No commands recorded yet.</span>
            ) : (
              history.map((h) => {
                const machine = h.machine ? machines.find((m) => m.id === h.machine) : undefined;
                const state = h.sessionId
                  ? (sessions.find((s) => s.id === h.sessionId)?.state ?? (h.ok ? "COMPLETED" : "FAILED"))
                  : h.ok === false
                    ? "FAILED"
                    : "COMPLETED";
                const durationColor = h.ok === false ? "var(--s-failed-fg)" : h.ok === null ? "var(--s-running-fg)" : "var(--text-1)";
                return (
                  <div key={h.id} className={styles.historyRow}>
                    <StateTag state={state} size="sm" iconOnly />
                    <span className={`${styles.historyCommand} mono`}>{h.command}</span>
                    <span className={styles.historyMachine}>{machine?.name ?? "—"}</span>
                    <span className={styles.historyDuration} style={{ color: durationColor }}>
                      {h.duration}
                    </span>
                  </div>
                );
              })
            )}
          </section>
        </div>

        <section className={styles.journal}>
          <div className={styles.sectionHead}>
            <span className={styles.sectionTitle}>Event journal</span>
            <span className={styles.sectionHint}>newest first</span>
          </div>
          <EventsList
            events={journalEvents.map((j) => j.event)}
            sessionNames={journalEvents.map((j) => j.sessionName)}
            showHead={false}
          />
        </section>
      </div>
    </div>
  );
}
