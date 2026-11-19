import { useState } from "react";
import { Link } from "react-router-dom";

import { ConfirmDialog } from "../../../components/ConfirmDialog";
import { StateIcon } from "../../../components/StateIcon";
import { formatSessionFact } from "../../../data/format";
import { clearsAt, clearsInText, isDone, useLifecycle } from "../../../data/lifecycle";
import type { Session } from "../../../data/types";
import { useSessionRemoval } from "../useSessionRemoval";
import styles from "./PhoneQuiet.module.css";

/** Done on the phone: swipe a row left (or focus it) for Hide and Delete…. */
export function PhoneDone({ sessions, machineName, now }: { sessions: Session[]; machineName: (id: string) => string; now: number }) {
  const life = useLifecycle();
  const removal = useSessionRemoval();
  const [deleting, setDeleting] = useState<Session | null>(null);
  const done = sessions.filter((session) => isDone(session) && !session.dismissed).sort((a, b) => (b.updatedAt ?? 0) - (a.updatedAt ?? 0));
  if (done.length === 0) return null;
  const notes = [...new Set(done.map((session) => removal.supportOf(session.id).note).filter(Boolean))];
  return (
    <section className={styles.section} aria-label="Done">
      <div className={styles.sectionHeadRow}>
        <h3 className={styles.sectionHead}>
          Done <span className={styles.muted}>{done.length}</span>
        </h3>
        <button type="button" className={styles.link} disabled={removal.busy} onClick={() => void removal.hide(done.map((session) => session.id))}>
          Hide all
        </button>
      </div>
      {notes.map((note) => (
        <p key={note} className={styles.versionNote} role="status">
          {note}
        </p>
      ))}
      {done.map((session) => {
        const support = removal.supportOf(session.id);
        const clears = clearsAt(session, life);
        const fate = !support.hide
          ? "stays until this Machine updates"
          : life.kept.includes(session.id)
            ? "kept"
            : clears !== null
              ? clearsInText(clears, now)
              : null;
        const how = session.state === "INTERRUPTED" ? "stopped" : (session.result?.summary ?? formatSessionFact(session));
        return (
          <SwipeRow
            key={session.id}
            name={session.name}
            busy={removal.busy}
            canHide={support.hide}
            canDelete={support.remove}
            onHide={() => void removal.hide([session.id])}
            onDelete={() => setDeleting(session)}
          >
            <StateIcon state={session.state} size={20} />
            <Link to={`/app/sessions/${encodeURIComponent(session.id)}`} className={styles.rowLink}>
              <span className={styles.rowTitle}>{session.name}</span>
              <span className={styles.rowMeta}>{[machineName(session.machine), how, fate].filter((part) => part && part !== "—").join(" · ")}</span>
            </Link>
          </SwipeRow>
        );
      })}
      {removal.error && (
        <p className={styles.empty} role="alert">
          {removal.error}
        </p>
      )}
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

const REVEAL = 148;

/**
 * A row that slides left to show Hide and Delete…. The buttons are always in
 * the page, so keyboard and screen-reader users reach them without swiping;
 * focusing one slides the row open too.
 */
export function SwipeRow({
  name,
  busy,
  canHide = true,
  canDelete = true,
  onHide,
  onDelete,
  children,
}: {
  name: string;
  busy: boolean;
  /** False when the Session's Machine runs a release that cannot do it. */
  canHide?: boolean;
  canDelete?: boolean;
  onHide: () => void;
  onDelete: () => void;
  children: React.ReactNode;
}) {
  const [offset, setOffset] = useState(0);
  const [start, setStart] = useState<{ x: number; y: number; base: number } | null>(null);
  const open = offset <= -REVEAL / 2;
  return (
    <div className={styles.swipe} data-open={open || undefined}>
      <div className={styles.swipeActions}>
        <button
          type="button"
          className={styles.swipeHide}
          disabled={busy || !canHide}
          onClick={onHide}
          onFocus={() => setOffset(-REVEAL)}
          aria-label={canHide ? `Hide ${name}` : `Hide ${name}: needs Sentinal 2.11 on its Machine`}
        >
          {canHide ? "Hide" : "Needs 2.11"}
        </button>
        <button
          type="button"
          className={styles.swipeDelete}
          disabled={busy || !canDelete}
          onClick={onDelete}
          onFocus={() => setOffset(-REVEAL)}
          aria-label={`Delete ${name}`}
        >
          Delete…
        </button>
      </div>
      <div
        className={styles.swipeFront}
        style={{ transform: `translateX(${offset}px)`, transition: start ? "none" : undefined }}
        onPointerDown={(event) => setStart({ x: event.clientX, y: event.clientY, base: offset })}
        onPointerMove={(event) => {
          if (!start) return;
          const dx = event.clientX - start.x;
          if (Math.abs(dx) < 6 || Math.abs(event.clientY - start.y) > Math.abs(dx)) return;
          setOffset(Math.max(-REVEAL, Math.min(0, start.base + dx)));
        }}
        onPointerUp={() => {
          if (!start) return;
          setStart(null);
          setOffset((value) => (value <= -REVEAL / 2 ? -REVEAL : 0));
        }}
        onPointerCancel={() => {
          setStart(null);
          setOffset((value) => (value <= -REVEAL / 2 ? -REVEAL : 0));
        }}
        onClickCapture={(event) => {
          // A tap on an open row closes it instead of opening the terminal.
          if (offset !== 0) {
            event.preventDefault();
            event.stopPropagation();
            setOffset(0);
          }
        }}
      >
        {children}
      </div>
    </div>
  );
}
