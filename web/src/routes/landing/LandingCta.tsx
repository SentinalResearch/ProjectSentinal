import { useRef, type RefObject } from "react";

import { SentinalMark } from "../../components/SentinalMark";
import { useReveal } from "../../hooks/useReveal";
import styles from "./LandingCta.module.css";

export interface LandingCtaProps {
  scrollContainerRef: RefObject<HTMLDivElement | null>;
  onSignIn: () => void;
  onSetup: () => void;
}

/** design_handoff_sentinal/design/SentinalLanding.dc.html — final CTA section (§7). */
export function LandingCta({ scrollContainerRef, onSignIn, onSetup }: LandingCtaProps) {
  const ref = useRef<HTMLDivElement | null>(null);
  const seen = useReveal(ref, scrollContainerRef, 0.75);

  return (
    <section ref={ref} className={`${styles.section} ${seen ? styles.seen : ""}`}>
      <div className={styles.markWrap}>
        <SentinalMark size={96} />
      </div>
      <span className={styles.heading}>
        Walk away.
        <br />
        <span className={styles.headingDim}>Sentinal is watching.</span>
      </span>
      <div className={styles.ctaRow}>
        <button type="button" className={styles.primaryCta} onClick={onSignIn}>
          Sign in
        </button>
        <button type="button" className={styles.secondaryCta} onClick={onSetup}>
          Set up this server
        </button>
      </div>
    </section>
  );
}
