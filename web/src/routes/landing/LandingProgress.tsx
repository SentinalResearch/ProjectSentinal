import { useEffect, useRef, useState, type RefObject } from "react";

import { prefersReducedMotion, useReveal } from "../../hooks/useReveal";
import styles from "./LandingProgress.module.css";

const TARGET_PCT = 68;

export interface LandingProgressProps {
  scrollContainerRef: RefObject<HTMLDivElement | null>;
}

/** design_handoff_sentinal/design/SentinalLanding.dc.html — progress section (§3). */
export function LandingProgress({ scrollContainerRef }: LandingProgressProps) {
  const ref = useRef<HTMLDivElement | null>(null);
  const seenNum = useReveal(ref, scrollContainerRef, 0.75);
  const seenNum2 = useReveal(ref, scrollContainerRef, 0.45);
  const [pct, setPct] = useState(prefersReducedMotion() ? TARGET_PCT : 0);

  useEffect(() => {
    if (!seenNum || prefersReducedMotion()) return;
    const t0 = performance.now();
    const dur = 1600;
    let raf = 0;
    const step = (t: number) => {
      const k = Math.min(1, (t - t0) / dur);
      const e = 1 - Math.pow(1 - k, 4);
      setPct(Math.round(TARGET_PCT * e));
      if (k < 1) raf = requestAnimationFrame(step);
    };
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
  }, [seenNum]);

  return (
    <section
      ref={ref}
      className={`${styles.section} ${seenNum ? styles.seen : ""} ${seenNum2 ? styles.seen2 : ""}`}
    >
      <span className={styles.overline}>QWEN TRAINING · RTX-PC</span>
      <span className={styles.pct}>
        {pct}
        <span className={styles.pctSign}>%</span>
      </span>
      <div className={styles.bar}>
        <div className={styles.barFill} style={{ width: `${pct}%` }} />
      </div>
      <div className={styles.stats}>
        <span>
          epoch <b>68/100</b>
        </span>
        <span>
          loss <b>0.41 → 0.37</b>
        </span>
        <span>
          GPU <b>96 %</b>
        </span>
        <span>
          VRAM <b>21.4 / 24 GB</b>
        </span>
      </div>
      <span className={styles.desc}>
        Progress, resources and a health check that shows its evidence: loss improving for 5 checks, last output 8s
        ago.
      </span>
    </section>
  );
}
