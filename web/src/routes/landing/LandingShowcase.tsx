import { useEffect, useRef, useState, type RefObject } from "react";

import { StateTag } from "../../components/StateTag";
import { prefersReducedMotion } from "../../hooks/useReveal";
import type { SessionState } from "../../protocol/SessionState";
import styles from "./LandingShowcase.module.css";

const QUESTIONS: [string, string][] = [
  ["What am I running?", "Every session on every machine, in one list."],
  ["How is it going?", "State, progress and health for each job."],
  ["Can I reach it?", "Answer a prompt or open the live terminal."],
  ["How did I run this last time?", "Searchable history of every command."],
];

const MOCK_ROWS: { state: SessionState; name: string; cmd: string; fact: string }[] = [
  { state: "RUNNING", name: "Qwen Training", cmd: "python train.py --config moe.yaml", fact: "epoch 68/100 · 1h31m" },
  { state: "RUNNING", name: "vLLM", cmd: "vllm serve Qwen/Qwen3-8B", fact: "serving on :8000" },
  { state: "FAILED", name: "pytest", cmd: "pytest tests/", fact: "3 failed · 182 passed" },
  { state: "IDLE", name: "shell", cmd: "~/Projects/TIM", fact: "—" },
  { state: "WAITING", name: "npm install", cmd: "Mac Mini", fact: "Continue? [y/N]" },
  { state: "STALLED", name: "Compression", cmd: "Mac Mini", fact: "no output 14m" },
];

const MOCK_HISTORY: { glyph: string; color: string; cmd: string; dur: string }[] = [
  { glyph: "✓", color: "var(--s-running-fg)", cmd: "cargo build", dur: "31s" },
  { glyph: "✗", color: "var(--s-failed-fg)", cmd: "pytest tests/", dur: "42s" },
  { glyph: "✓", color: "var(--s-running-fg)", cmd: "python benchmark.py", dur: "18m" },
  { glyph: "●", color: "var(--s-running-fg)", cmd: "python train.py", dur: "running" },
];

/** Resting progress under prefers-reduced-motion: mock fully flat/visible (needs intro >= 0.22), no section dimmed yet (needs p < 0.28). */
const REDUCED_MOTION_P = 0.25;

function easeOutCubic(x: number): number {
  return 1 - Math.pow(1 - x, 3);
}

function computeProgress(scrollEl: HTMLElement, sectionEl: HTMLElement): number {
  const vh = scrollEl.clientHeight;
  const r = sectionEl.getBoundingClientRect();
  const top = scrollEl.getBoundingClientRect().top;
  const total = sectionEl.offsetHeight - vh;
  return Math.max(0, Math.min(1, (top + 60 - r.top) / Math.max(1, total)));
}

export interface LandingShowcaseProps {
  scrollContainerRef: RefObject<HTMLDivElement | null>;
}

/** design_handoff_sentinal/design/SentinalLanding.dc.html — showcase section (§2), the "4 questions" sticky scroll-through. */
export function LandingShowcase({ scrollContainerRef }: LandingShowcaseProps) {
  const sectionRef = useRef<HTMLDivElement | null>(null);
  const [p, setP] = useState(prefersReducedMotion() ? REDUCED_MOTION_P : 0);

  useEffect(() => {
    if (prefersReducedMotion()) return;
    const scrollEl = scrollContainerRef.current;
    const sectionEl = sectionRef.current;
    if (!scrollEl || !sectionEl) return;

    let raf = 0;
    const measure = () => setP(computeProgress(scrollEl, sectionEl));
    const onScroll = () => {
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(measure);
    };
    measure();
    scrollEl.addEventListener("scroll", onScroll, { passive: true });
    window.addEventListener("resize", onScroll);
    return () => {
      cancelAnimationFrame(raf);
      scrollEl.removeEventListener("scroll", onScroll);
      window.removeEventListener("resize", onScroll);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const intro = Math.min(1, p / 0.22);
  const ei = easeOutCubic(intro);
  const step = p < 0.28 ? -1 : p < 0.46 ? 0 : p < 0.64 ? 1 : p < 0.82 ? 2 : 3;
  const shownStep = Math.max(0, step);

  const mockO = 0.35 + 0.65 * ei;
  const mockT = `translateY(${(1 - ei) * 60}px) scale(${0.82 + 0.18 * ei}) rotateX(${(1 - ei) * 24}deg)`;

  const on = (k: number) => (step < 0 || step === k ? 1 : 0.18);
  const dim = { list: on(0), state: on(1), reach: on(2), hist: on(3) };
  const reachT = step === 2 ? "scale(1.04)" : "none";
  const histT = step === 3 ? "translateY(-4px)" : "none";

  return (
    <section ref={sectionRef} className={styles.section}>
      <div className={styles.sticky}>
        <div className={styles.steps}>
          {QUESTIONS.map(([q, a], i) => {
            const active = i === shownStep;
            const before = i < shownStep;
            return (
              <div
                key={q}
                className={styles.step}
                style={{
                  opacity: active ? ei : 0,
                  transform: active ? "none" : before ? "translateY(-24px)" : "translateY(24px)",
                }}
              >
                <span className={styles.stepN}>{String(i + 1).padStart(2, "0")} / 04</span>
                <span className={styles.stepQ}>{q}</span>
                <span className={styles.stepA}>{a}</span>
              </div>
            );
          })}
        </div>

        <div className={styles.mockFrame} style={{ transform: mockT, opacity: mockO }}>
          <div className={styles.mockInner}>
            <div className={styles.mockSidebar}>
              <div className={styles.mockBrand}>
                <svg viewBox="0 0 24 24" width={14} height={14} aria-hidden="true">
                  <path d="M5 22L13.5 2" stroke="var(--text-1)" strokeWidth="3.4" strokeLinecap="square" fill="none" />
                  <circle cx="18" cy="18.5" r="3.4" fill="var(--accent)" />
                </svg>
                <span>Sentinal</span>
              </div>
              <span className={styles.mockNavActive}>Overview</span>
              <span className={styles.mockNav}>Split view</span>
              <span className={styles.mockNav}>History</span>
              <span className={styles.mockNav}>Recipes</span>
              <span className={styles.mockMachinesLabel}>MACHINES</span>
              <span className={styles.mockMachine}>
                <span className={styles.mockDotOnline} />
                RTX-PC
              </span>
              <span className={styles.mockMachine}>
                <span className={styles.mockDotOnline} />
                Mac Mini
              </span>
              <span className={`${styles.mockMachine} ${styles.offline}`}>
                <span className={styles.mockDotOffline} />
                MacBook
              </span>
            </div>

            <div className={styles.mockMain}>
              <span className={styles.mockTitle}>Overview</span>
              <div className={styles.mockBody}>
                <div className={styles.mockList}>
                  <div className={styles.mockListHead}>
                    RTX-PC <span>· Ryzen 9 · RTX 4090</span>
                  </div>
                  {MOCK_ROWS.map((r) => (
                    <div key={r.name} className={styles.mockRow}>
                      <span className={styles.mockRowTag} style={{ opacity: dim.state }}>
                        <StateTag state={r.state} size="sm" />
                      </span>
                      <span className={styles.mockRowName} style={{ opacity: dim.list }}>
                        <span>{r.name}</span>
                        <span>{r.cmd}</span>
                      </span>
                      <span className={styles.mockRowFact} style={{ opacity: dim.state }}>
                        {r.fact}
                      </span>
                    </div>
                  ))}
                </div>

                <div className={styles.mockAside}>
                  <div className={styles.mockWaiting} style={{ opacity: dim.reach, transform: reachT }}>
                    <span className={styles.mockWaitingLabel}>MAC MINI · NPM INSTALL</span>
                    <span className={styles.mockWaitingPrompt}>Continue? [y/N]</span>
                    <div className={styles.mockWaitingKeys}>
                      <span className={styles.mockKeyY}>y</span>
                      <span className={styles.mockKeyN}>n</span>
                      <span className={styles.mockKeyOpen}>Open terminal</span>
                    </div>
                  </div>
                  <div className={styles.mockTerm} style={{ opacity: dim.reach }}>
                    <span>epoch 66/100 loss 0.39</span>
                    <span>epoch 67/100 loss 0.38</span>
                    <span>epoch 68/100 loss 0.37</span>
                  </div>
                  <div className={styles.mockHistory} style={{ opacity: dim.hist, transform: histT }}>
                    <span className={styles.mockHistoryHead}>History</span>
                    {MOCK_HISTORY.map((h) => (
                      <div key={h.cmd} className={styles.mockHistoryRow}>
                        <span style={{ color: h.color }}>{h.glyph}</span>
                        <span>{h.cmd}</span>
                        <span>{h.dur}</span>
                      </div>
                    ))}
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>

        <div className={styles.dots}>
          {QUESTIONS.map((q, i) => (
            <span
              key={q[0]}
              className={styles.dot}
              style={{
                width: i === shownStep && step >= 0 ? "32px" : "8px",
                background: i <= step ? "var(--text-1)" : "var(--line-strong)",
              }}
            />
          ))}
        </div>
      </div>
    </section>
  );
}
