import { useEffect, useState } from "react";

import { prefersReducedMotion } from "../../hooks/useReveal";
import styles from "./LandingHero.module.css";

export interface LandingHeroProps {
  cueVisible: boolean;
  onSignIn: () => void;
  onSetup: () => void;
  onScrollDown: () => void;
}

/** The 120px animated mark: draw-in slash, pop-in dot, then an idle ping ring. */
function HeroMark() {
  return (
    <div className={styles.markWrap}>
      <svg viewBox="0 0 24 24" width={120} height={120} className={styles.mark} aria-label="Sentinal" role="img">
        <path
          className={`${styles.markSlash} tg-motion`}
          d="M5 22L13.5 2"
          pathLength={1}
          fill="none"
          stroke="var(--text-1)"
          strokeWidth="3.4"
          strokeLinecap="square"
        />
        <circle
          className={`${styles.markRing} tg-motion`}
          cx="18"
          cy="18.5"
          r="3.4"
          fill="none"
          stroke="var(--accent)"
          strokeWidth="1"
        />
        <circle className={`${styles.markDot} tg-motion`} cx="18" cy="18.5" r="3.4" fill="var(--accent)" />
      </svg>
    </div>
  );
}

/** design_handoff_sentinal/design/SentinalLanding.dc.html — hero section (§1). */
export function LandingHero({ cueVisible, onSignIn, onSetup, onScrollDown }: LandingHeroProps) {
  const [loaded, setLoaded] = useState(prefersReducedMotion);

  useEffect(() => {
    if (loaded) return;
    const t = setTimeout(() => setLoaded(true), 60);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <section className={`${styles.hero} ${loaded ? styles.loaded : ""}`}>
      <HeroMark />
      <h1 className={styles.title}>
        <span className={styles.titleLine}>Every terminal,</span>
        <span className={styles.titleLine}>every machine,</span>
        <span className={styles.titleLine}>at a glance.</span>
      </h1>
      <p className={styles.subtext}>
        A local-first control plane for the long jobs you run on your own machines. Start something, walk away,
        check it from any browser.
      </p>
      <div className={styles.ctaRow}>
        <button type="button" className={styles.primaryCta} onClick={onSignIn}>
          Sign in
        </button>
        <button type="button" className={styles.secondaryCta} onClick={onSetup}>
          Set up this server
        </button>
      </div>
      <button
        type="button"
        aria-label="Scroll"
        className={`${styles.scrollCue} ${cueVisible ? styles.visible : ""}`}
        onClick={onScrollDown}
      >
        SCROLL
        <span className={`${styles.cueLine} tg-motion`} />
      </button>
    </section>
  );
}
