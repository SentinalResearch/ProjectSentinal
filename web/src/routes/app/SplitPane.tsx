import type { MouseEvent } from "react";
import { useNavigate } from "react-router-dom";

import { StateTag } from "../../components/StateTag";
import { CaptureBadge } from "../../components/CaptureBadge";
import { captureCapabilities, captureOf, exitUnknown } from "../../data/capture";
import { formatSessionFact } from "../../data/format";
import { machineStatusText } from "../../data/machine-status";
import type { Machine, Session } from "../../data/types";
import { TerminalPane } from "../../terminal/TerminalPane";
import { ReducedCapturePanel } from "./ReducedCapturePanel";
import styles from "./SplitPane.module.css";

export interface SplitPaneProps {
  session: Session;
  machine: Machine | undefined;
  active: boolean;
  /** `"span 2"` when this is the first of exactly 3 panes (design_handoff_sentinal/design/SentinalSplit.dc.html). */
  gridRow: string;
  onFocus: () => void;
  onClose: () => void;
}

export function SplitPane({ session, machine, active, gridRow, onFocus, onClose }: SplitPaneProps) {
  const navigate = useNavigate();

  function openDetails(e: MouseEvent) {
    e.stopPropagation();
    navigate(`/app/sessions/${session.id}`);
  }

  function close(e: MouseEvent) {
    e.stopPropagation();
    onClose();
  }

  return (
    <section
      className={styles.pane}
      style={{ gridRow, boxShadow: active ? "inset 0 2px 0 var(--accent)" : "none" }}
      onClick={onFocus}
    >
      <div className={styles.head}>
        <StateTag state={session.state} size="sm" exitUnknown={exitUnknown(session)} />
        <CaptureBadge capture={session.capture ?? "FULL"} origin={session.origin} />
        <span className={styles.name}>{session.name}</span>
        <span className={styles.fact}>
          {machine?.name ?? session.machine} · {machine ? machineStatusText(machine, Date.now()) : "offline"} · {formatSessionFact(session)}
        </span>
        <button type="button" className={styles.detailsBtn} title="Open session" onClick={openDetails}>
          Details
        </button>
        <button type="button" className={styles.closeBtn} title="Close pane" onClick={close}>
          ×
        </button>
      </div>
      {captureCapabilities(captureOf(session)).canAttach ? (
        <TerminalPane
          sessionId={session.id}
          session={session}
          height="fill"
          showKeyBar={false}
          className={styles.terminalWrap}
        />
      ) : (
        <div className={styles.unavailable}>
          <ReducedCapturePanel session={session} />
        </div>
      )}
    </section>
  );
}
