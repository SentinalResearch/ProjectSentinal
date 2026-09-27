import { useRef, type RefObject } from "react";

import { StateTag } from "../../components/StateTag";
import { useReveal } from "../../hooks/useReveal";
import type { SessionState } from "../../protocol/SessionState";
import styles from "./LandingStates.module.css";

const STATES: SessionState[] = [
  "CREATED",
  "IDLE",
  "RUNNING",
  "WAITING",
  "STALLED",
  "COMPLETED",
  "FAILED",
  "INTERRUPTED",
  "DISCONNECTED",
];

export interface LandingStatesProps {
  scrollContainerRef: RefObject<HTMLDivElement | null>;
}

/** design_handoff_sentinal/design/SentinalLanding.dc.html — states section (§6): all 9 StateTags pop in, 60ms stagger. */
export function LandingStates({ scrollContainerRef }: LandingStatesProps) {
  const ref = useRef<HTMLDivElement | null>(null);
  const seen = useReveal(ref, scrollContainerRef, 0.75);

  return (
    <section ref={ref} className={`${styles.section} ${seen ? styles.seen : ""}`}>
      <span className={styles.heading}>Nine states. Readable from across the room.</span>
      <div className={styles.tags}>
        {STATES.map((state, i) => (
          <span key={state} className={styles.tagWrap} style={{ transitionDelay: `${(0.15 + i * 0.06).toFixed(2)}s` }}>
            <StateTag state={state} />
          </span>
        ))}
      </div>
      <span className={styles.desc}>Every state has its own color, icon and label, so none depends on color alone.</span>
    </section>
  );
}
