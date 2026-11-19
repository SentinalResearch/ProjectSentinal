import { useId, useState } from "react";

import { ConfirmDialog } from "../../../components/ConfirmDialog";
import { StateIcon } from "../../../components/StateIcon";
import { formatSessionFact } from "../../../data/format";
import {
  AUTO_CLEAR_LABEL,
  clearsAt,
  clearsInText,
  isDone,
  lifecycle,
  useLifecycle,
  type AutoClear,
} from "../../../data/lifecycle";
import type { Session } from "../../../data/types";
import type { RemovalSupport } from "../../../data/removal";
import { useSessionRemoval } from "../useSessionRemoval";
import styles from "./GlassDashboard.module.css";

const ORDER: AutoClear[] = ["1h", "1d", "never"];

/**
 * Design L1: finished and stopped terminals, newest first, each with Open,
 * Hide and Delete…; the header sets auto-clear and hides them all at once.
 * Failures stay under Needs you.
 */
export function DoneSection({
  sessions,
  machineName,
  now,
  onOpen,
}: {
  sessions: Session[];
  machineName: (id: string) => string;
  now: number;
  onOpen: (id: string) => void;
}) {
  const life = useLifecycle();
  const removal = useSessionRemoval();
  const [deleting, setDeleting] = useState<Session | null>(null);
  const done = sessions.filter((session) => isDone(session) && !session.dismissed).sort((a, b) => (b.updatedAt ?? 0) - (a.updatedAt ?? 0));
  if (done.length === 0) return null;
  const notes = [...new Set(done.map((session) => removal.supportOf(session.id).note).filter(Boolean))];
  const next = ORDER[(ORDER.indexOf(life.autoClear) + 1) % ORDER.length];

  return (
    <section className={styles.section} aria-labelledby="done-heading">
      <div className={styles.sectionHead}>
        <h2 id="done-heading" className={styles.sectionTitle}>
          Done <span className={styles.muted}>{done.length}</span>
        </h2>
        <span className={styles.sectionTools}>
          <button
            type="button"
            className={styles.linkQuiet}
            onClick={() => lifecycle.setAutoClear(next)}
            title="Finished terminals hide themselves; failures wait until you've opened them. Delete is never automatic."
          >
            Auto-clear: {AUTO_CLEAR_LABEL[life.autoClear]}
          </button>
          <button type="button" className={styles.linkAccent} disabled={removal.busy} onClick={() => void removal.hide(done.map((session) => session.id))}>
            Hide all finished
          </button>
        </span>
      </div>
      {removal.error && (
        <p className={styles.needNote} role="alert">
          {removal.error}
        </p>
      )}
      {notes.map((note) => (
        <p key={note} className={styles.needNote} role="status">
          {note}
        </p>
      ))}
      {done.map((session) => (
        <DoneRow
          key={session.id}
          session={session}
          support={removal.supportOf(session.id)}
          where={machineName(session.machine)}
          clears={clearsAt(session, life)}
          kept={life.kept.includes(session.id)}
          now={now}
          busy={removal.busy}
          onOpen={() => onOpen(session.id)}
          onHide={() => void removal.hide([session.id])}
          onDelete={() => setDeleting(session)}
        />
      ))}
      <ConfirmDialog
        open={deleting !== null}
        overline="Delete terminal"
        question={`Permanently delete ${deleting?.name ?? ""}?`}
        consequence="Its saved output, Commands and Events are erased from this Machine. This cannot be undone. Hide keeps it restorable."
        confirmLabel="Delete"
        focusCancel
        onConfirm={() => {
          const target = deleting;
          setDeleting(null);
          if (target) void removal.remove([target.id]);
        }}
        onCancel={() => setDeleting(null)}
      />
    </section>
  );
}

function DoneRow({
  session,
  support,
  where,
  clears,
  kept,
  now,
  busy,
  onOpen,
  onHide,
  onDelete,
}: {
  session: Session;
  support: RemovalSupport;
  where: string;
  clears: number | null;
  kept: boolean;
  now: number;
  busy: boolean;
  onOpen: () => void;
  onHide: () => void;
  onDelete: () => void;
}) {
  const titleId = `done-${useId()}`;
  const stop = (handler: () => void) => (event: { stopPropagation(): void }) => {
    event.stopPropagation();
    handler();
  };
  const how = session.state === "INTERRUPTED" ? "stopped" : (session.result?.summary ?? formatSessionFact(session));
  const fate = !support.hide
    ? "stays until this Machine updates"
    : kept
      ? "kept"
      : clears !== null
        ? clearsInText(clears, now)
        : null;
  return (
    <article className={styles.doneRow} aria-labelledby={titleId} onClick={onOpen}>
      <StateIcon state={session.state} size={20} />
      <div className={styles.needBody}>
        <h3 className={styles.needTitle} id={titleId}>
          <button type="button" className={styles.titleButton} onClick={stop(onOpen)}>
            {session.name}
          </button>
        </h3>
        <p className={styles.needMeta}>{[where, how, fate].filter((part) => part && part !== "—").join(" · ")}</p>
      </div>
      <div className={styles.doneActions}>
        <button type="button" className={styles.pill} onClick={stop(onOpen)} aria-label={`Open ${session.name}`}>
          Open
        </button>
        <button
          type="button"
          className={styles.pill}
          disabled={busy || !support.hide}
          onClick={stop(onHide)}
          aria-label={support.hide ? `Hide ${session.name}` : `Hide ${session.name}: needs Sentinal 2.11 on its Machine`}
        >
          {support.hide ? "Hide" : "Hide · needs 2.11"}
        </button>
        <button
          type="button"
          className={styles.dangerQuiet}
          disabled={busy || !support.remove}
          onClick={stop(onDelete)}
          aria-label={`Delete ${session.name}`}
        >
          Delete…
        </button>
      </div>
    </article>
  );
}
