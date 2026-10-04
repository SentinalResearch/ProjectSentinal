import { Button } from "../../components/Button";
import { extractPort, formatLearningRate, lossOf, type SessionKind } from "../../data/format";
import type { Session } from "../../data/types";
import styles from "./SessionTopBlock.module.css";

const TREND = { IMPROVING: "↓ improving", STABLE: "→ stable", WORSENING: "↑ worsening" } as const;

export interface SessionTopBlockProps {
  session: Session;
  kind: SessionKind;
  interrupted: boolean;
  lastCheckpoint: string | null;
  /** False for reduced capture: no answer, re-run, or output controls. */
  canSendInput: boolean;
  onAnswer: (value: "y" | "n") => void;
  onRerun: () => void;
  onShowOutput: () => void;
}

/**
 * The kind-specific block above the terminal on Session Detail
 * (design_handoff_sentinal/design/SentinalSession.dc.html). `interrupted` always
 * wins over `kind` — an interrupted training/server/etc. session shows the
 * interrupted notice instead of its normal top block.
 */
export function SessionTopBlock({
  session,
  kind,
  interrupted,
  lastCheckpoint,
  canSendInput,
  onAnswer,
  onRerun,
  onShowOutput,
}: SessionTopBlockProps) {
  if (interrupted) {
    return (
      <section className={`${styles.panel} ${styles.interrupted}`}>
        <span className={styles.interruptedText}>
          Interrupted with Ctrl‑C.
          {lastCheckpoint && (
            <>
              {" "}
              Last checkpoint <span className="mono">{lastCheckpoint}</span>.
            </>
          )}
        </span>
      </section>
    );
  }

  if (kind === "training" && session.progress) {
    const { current, total, unit } = session.progress;
    const pct = Math.round((current / total) * 100);
    const gpuPct = session.metrics?.gpuPct;
    const vram = session.metrics?.vramGB;
    const vramTotal = session.metrics?.vramTotalGB;
    const vramPct = vram !== undefined && vramTotal ? Math.min(100, (vram / vramTotal) * 100) : undefined;
    const loss = lossOf(session);
    const training = session.training;
    return (
      <section className={styles.training}>
        <div className={styles.percentCol}>
          <div className={styles.percentRow}>
            <span className={styles.percentValue}>
              {pct}
              <span className={styles.percentSign}>%</span>
            </span>
            <span className={styles.percentCaption}>
              {unit} {current}/{total}
              <br />
              {session.runningFor && `running ${session.runningFor}`}
            </span>
          </div>
          <div className={styles.progressTrack}>
            <div className={styles.progressFill} style={{ width: `${pct}%` }} />
            <div className={styles.progressTicks} />
          </div>
        </div>

        {loss && (
          <div className={styles.metricCol}>
            <span className={styles.metricLabel}>LOSS</span>
            <span className={styles.metricValue}>{loss.to}</span>
            <span className={styles.metricCaption}>
              {[
                loss.from !== undefined ? `from ${loss.from}` : null,
                training && training.trend !== "UNKNOWN" ? `${TREND[training.trend]}` : null,
                training?.bestValLoss !== undefined ? `best val ${training.bestValLoss}` : null,
                training?.learningRate !== undefined ? `lr ${formatLearningRate(training.learningRate)}` : null,
              ]
                .filter(Boolean)
                .join(" · ")}
            </span>
          </div>
        )}

        {gpuPct !== undefined && (
          <div className={styles.metricCol}>
            <span className={styles.metricLabel}>GPU</span>
            <span className={styles.metricValue}>{gpuPct} %</span>
            <div className={styles.meterTrack}>
              <div className={styles.meterFill} style={{ width: `${gpuPct}%` }} />
            </div>
          </div>
        )}

        {vram !== undefined && vramTotal !== undefined && (
          <div className={styles.metricCol}>
            <span className={styles.metricLabel}>VRAM</span>
            <span className={styles.metricValue}>
              {vram}
              <span className={styles.metricUnit}> / {vramTotal} GB</span>
            </span>
            <div className={styles.meterTrack}>
              <div className={styles.meterFill} style={{ width: `${vramPct}%` }} />
              <div className={styles.meterTick} style={{ left: "95%" }} />
            </div>
          </div>
        )}
      </section>
    );
  }

  if (kind === "failed" && session.result) {
    const { failed, passed, duration, summary } = session.result;
    return (
      <section className={`${styles.panel} ${styles.failed}`}>
        <span className={styles.panelOverline} style={{ color: "var(--s-failed-fg)" }}>
          FAILURE SUMMARY
        </span>
        <div className={styles.failedNums}>
          <span className={styles.failedBig}>{failed} failed</span>
          <span className={styles.failedSub}>
            {passed} passed · {duration}
          </span>
        </div>
        <span className={styles.failedSummary}>{summary}</span>
        {canSendInput && (
          <div className={styles.failedActions}>
            <Button variant="primary" onClick={onRerun}>
              Run again
            </Button>
            <Button variant="secondary" onClick={onShowOutput}>
              Show output
            </Button>
          </div>
        )}
      </section>
    );
  }

  if (kind === "waiting" && session.prompt) {
    return (
      <section className={`${styles.panel} ${styles.waiting}`}>
        <div className={styles.waitingLeft}>
          <span className={styles.panelOverline} style={{ color: "var(--s-waiting-fg)" }}>
            WAITING FOR INPUT
          </span>
          <span className={styles.waitingPrompt}>{session.prompt}</span>
          <span className={styles.waitingCaption}>
            {canSendInput
              ? "Default is N. Your answer is typed into the terminal followed by Enter."
              : "Sentinal has no terminal input for this Session. Answer where the process runs."}
          </span>
        </div>
        {canSendInput && (
          <div className={styles.yn}>
            <button type="button" className={styles.ynKey} onClick={() => onAnswer("y")}>
              y
            </button>
            <button type="button" className={styles.ynKey} onClick={() => onAnswer("n")}>
              n
            </button>
          </div>
        )}
      </section>
    );
  }

  if (kind === "stalled") {
    const [big, sub] = (session.details ?? "no output").split(", ");
    const confidence = session.health?.confidence;
    return (
      <section className={`${styles.panel} ${styles.stalled}`}>
        <div className={styles.stalledLeft}>
          <span className={styles.panelOverline} style={{ color: "var(--s-stalled-fg)" }}>
            POSSIBLY STALLED{confidence !== undefined ? ` · ${Math.round(confidence * 100)} % CONFIDENCE` : ""}
          </span>
          <span className={styles.stalledBig}>{big}</span>
        </div>
        {sub && <span className={styles.stalledSub}>{sub}</span>}
      </section>
    );
  }

  if (kind === "server") {
    const port = extractPort(session);
    return (
      <section className={styles.server}>
        {port && <span className={styles.serverPort}>:{port}</span>}
        {session.details && <span className={styles.serverCaption}>{session.details}</span>}
      </section>
    );
  }

  return null;
}
