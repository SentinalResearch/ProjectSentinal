import { useEffect, useRef, useState, type RefObject } from "react";

import { SentinalMark } from "../../components/SentinalMark";
import { StateTag } from "../../components/StateTag";
import { prefersReducedMotion, useReveal } from "../../hooks/useReveal";
import styles from "./LandingPhone.module.css";

const FEATS: [string, string, string][] = [
  ["01", "Glance.", "Needs you rises to the top: failed, waiting, stalled."],
  ["02", "Answer.", "y / n on prompts without opening a terminal."],
  ["03", "Act.", "Ctrl‑C with a confirm step, never one accidental tap."],
  ["04", "Get told.", "Notifications per session: finished, error, stalled, checkpoint."],
];

export interface LandingPhoneProps {
  scrollContainerRef: RefObject<HTMLDivElement | null>;
}

/** design_handoff_sentinal/design/SentinalLanding.dc.html — phone section (§4): a scripted, timed sequence once revealed. */
export function LandingPhone({ scrollContainerRef }: LandingPhoneProps) {
  const ref = useRef<HTMLDivElement | null>(null);
  const seen = useReveal(ref, scrollContainerRef, 0.7);
  const [ph, setPh] = useState(prefersReducedMotion() ? 3 : 0);

  useEffect(() => {
    if (!seen || prefersReducedMotion()) return;
    const timers = [
      setTimeout(() => setPh(1), 1400),
      setTimeout(() => setPh(2), 3000),
      setTimeout(() => setPh(3), 3250),
    ];
    return () => timers.forEach(clearTimeout);
  }, [seen]);

  const notifShown = ph >= 1 && ph < 3;
  const npmRunning = ph >= 3;
  const needsCount = ph >= 3 ? 2 : 3;

  return (
    <section ref={ref} className={`${styles.section} ${seen ? styles.seen : ""}`}>
      <div className={styles.inner}>
        <div className={styles.copy}>
          <span className={styles.overline}>on your phone</span>
          <span className={styles.heading}>Walk away. Answer from anywhere.</span>
          <span className={styles.desc}>
            Install Sentinal to your home screen. Get a notification when a job fails or waits for input, answer y /
            n in one tap, and open the real terminal when you need it.
          </span>
          <div className={styles.feats}>
            {FEATS.map(([n, k, v], i) => (
              <div key={n} className={styles.feat} style={{ transitionDelay: `${(0.35 + i * 0.1).toFixed(2)}s` }}>
                <span className={styles.featN}>{n}</span>
                <span className={styles.featBody}>
                  <b>{k}</b> <span>{v}</span>
                </span>
              </div>
            ))}
          </div>
        </div>

        <div className={styles.deviceWrap}>
          <div className={styles.glow} />
          <div className={styles.device}>
            <div className={styles.screen}>
              <div className={styles.notch} />
              <div className={`${styles.notification} ${notifShown ? styles.show : ""}`}>
                <span className={styles.notifIcon}>
                  <SentinalMark size={16} color="#F2F2F0" accent="#3D5AFE" />
                </span>
                <span className={styles.notifBody}>
                  <span className={styles.notifTitleRow}>
                    <span>Sentinal</span>
                    <span>now</span>
                  </span>
                  <span className={styles.notifText}>Mac Mini · npm install: waiting for input — Continue? [y/N]</span>
                </span>
              </div>

              <div className={styles.screenBody}>
                <div className={styles.screenHead}>
                  <span>Sessions</span>
                  <SentinalMark size={16} />
                </div>
                <div className={styles.machineRow}>
                  <div className={styles.machineCell}>
                    <b>RTX-PC</b>
                    <span>Online · 4</span>
                  </div>
                  <div className={styles.machineCell}>
                    <b>Mac Mini</b>
                    <span>Online · 2</span>
                  </div>
                  <div className={`${styles.machineCell} ${styles.offline}`}>
                    <b>MacBook</b>
                    <span>Offline 14m</span>
                  </div>
                </div>
              </div>

              <div className={styles.list}>
                <div className={styles.listHead}>
                  <span>Needs you</span>
                  <span>{needsCount}</span>
                </div>

                <div className={styles.npmCard} style={{ background: npmRunning ? "color-mix(in srgb, var(--s-running) 8%, transparent)" : "transparent" }}>
                  <div className={styles.npmTop}>
                    <StateTag state={npmRunning ? "RUNNING" : "WAITING"} size="sm" />
                    <b>npm install</b>
                    <span>Mac Mini</span>
                  </div>
                  <div className={styles.npmKeys}>
                    <span className={styles.npmPrompt}>{npmRunning ? "answered y" : "Continue? [y/N]"}</span>
                    <span
                      className={styles.npmKey}
                      style={{
                        transform: ph === 2 ? "scale(.9)" : "none",
                        boxShadow: ph === 2 ? "0 0 0 3px var(--accent)" : "none",
                      }}
                    >
                      y
                    </span>
                    <span className={styles.npmKey}>n</span>
                  </div>
                </div>

                <div className={styles.item}>
                  <div className={styles.itemTop}>
                    <StateTag state="FAILED" size="sm" />
                    <b>pytest</b>
                    <span>RTX-PC</span>
                  </div>
                  <span className={styles.itemFact}>3 failed · 182 passed · 42s</span>
                </div>

                <div className={styles.item}>
                  <div className={styles.itemTop}>
                    <StateTag state="STALLED" size="sm" />
                    <b>Compression</b>
                    <span>Mac Mini</span>
                  </div>
                  <span className={styles.itemFact}>no output 14m · CPU 0 %</span>
                </div>

                <div className={styles.trainItem}>
                  <div className={styles.trainTop}>
                    <b>Qwen Training</b>
                    <StateTag state="RUNNING" size="sm" />
                  </div>
                  <div className={styles.trainStat}>
                    <span className={styles.trainPct}>68 %</span>
                    <span className={styles.trainFact}>epoch 68/100 · 1h31m</span>
                  </div>
                  <div className={styles.trainBar}>
                    <div className={styles.trainBarFill} />
                  </div>
                </div>
              </div>

              <div className={styles.homeIndicator}>
                <span />
              </div>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
