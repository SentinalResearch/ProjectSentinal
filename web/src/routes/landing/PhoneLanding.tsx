import { useNavigate } from "react-router-dom";

import { SentinalMark } from "../../components/SentinalMark";
import { StateTag } from "../../components/StateTag";
import { useTheme } from "../../context/ThemeContext";
import type { SessionState } from "../../protocol/SessionState";
import styles from "./PhoneLanding.module.css";

const NEEDS: { state: SessionState; machine: string; title: string; fact: string }[] = [
  { state: "WAITING", machine: "Mac Mini", title: "npm install", fact: "Continue? [y/N]" },
  { state: "FAILED", machine: "RTX-PC", title: "pytest", fact: "3 failed · 182 passed" },
  { state: "STALLED", machine: "Mac Mini", title: "Compression", fact: "No output for 12 min" },
];

const RUNNING: { state: SessionState; duration: string; title: string; command: string }[] = [
  { state: "RUNNING", duration: "1h31m", title: "Qwen Training", command: "python train.py --config moe.yaml" },
  { state: "RUNNING", duration: "2h04m", title: "vLLM", command: "vllm serve Qwen/Qwen3-8B --port 8000" },
  { state: "IDLE", duration: "—", title: "shell", command: "~/Projects/TIM" },
];

/** A1 Landing + A2 Features on phones (MOBILE_SPEC.md): native rows, no scaled desktop mocks. */
export function PhoneLanding() {
  const navigate = useNavigate();
  const { theme, toggleTheme } = useTheme();
  return (
    <div className={styles.page}>
      <header className={styles.header}>
        <span className={styles.brand}>
          <SentinalMark size={22} />
          <span>Sentinal</span>
        </span>
        <button type="button" className={styles.iconButton} aria-label={theme === "light" ? "Dark theme" : "Light theme"} onClick={toggleTheme}>
          ◐
        </button>
        <button type="button" className={styles.textButton} onClick={() => navigate("/login")}>
          Sign in
        </button>
      </header>

      <section className={styles.hero}>
        <span className={styles.overline}>local-first · self-hosted</span>
        <h1 className={styles.headline}>Every terminal, every machine, at a glance.</h1>
        <p className={styles.lede}>
          A local-first control plane for the long jobs you run on your own machines. Start something, walk away, check it from any browser.
        </p>
        <div className={styles.actions} role="group" aria-label="Get started">
          <button type="button" className={styles.primary} onClick={() => navigate("/login")}>
            Sign in
          </button>
          <button type="button" className={styles.outline} onClick={() => navigate("/setup")}>
            Set up this server
          </button>
        </div>
        <ul className={styles.preview} aria-label="Needs you preview">
          {NEEDS.map((item) => (
            <li key={item.title} className={styles.previewRow}>
              <span className={styles.rowLine}>
                <StateTag state={item.state} size="sm" />
                <span className={styles.meta}>{item.machine}</span>
              </span>
              <strong>{item.title}</strong>
              <span className={`${styles.meta} mono`}>{item.fact}</span>
            </li>
          ))}
        </ul>
      </section>

      <section className={styles.feature}>
        <span className={styles.overline}>01 / 04</span>
        <h2 className={styles.featureTitle}>What am I running?</h2>
        <p className={styles.lede}>Every session on every machine, in one list. What needs you comes first.</p>
        <ul className={styles.list}>
          {RUNNING.map((item) => (
            <li key={item.title} className={styles.previewRow}>
              <span className={styles.rowLine}>
                <StateTag state={item.state} size="sm" />
                <span className={`${styles.meta} mono`}>{item.duration}</span>
              </span>
              <strong>{item.title}</strong>
              <span className={`${styles.meta} mono`}>{item.command}</span>
            </li>
          ))}
        </ul>
      </section>

      <section className={styles.feature}>
        <span className={styles.overline}>02 / 04</span>
        <h2 className={styles.featureTitle}>How is it going?</h2>
        <div className={styles.progressBlock}>
          <span className={styles.bigNumber}>68 %</span>
          <span className={`${styles.meta} mono`}>epoch 68/100 · loss 0.37 ↓</span>
          <div className={styles.track}>
            <div className={styles.fill} />
          </div>
        </div>
      </section>

      <section className={styles.feature}>
        <span className={styles.overline}>03 / 04</span>
        <h2 className={styles.featureTitle}>Walk away. Answer from anywhere.</h2>
        <p className={styles.lede}>A push tells you when a job waits, fails, or stalls. Tap it to land on that exact session and answer y or n.</p>
      </section>

      <section className={styles.feature}>
        <span className={styles.overline}>04 / 04</span>
        <h2 className={styles.featureTitle}>Run it the same way, every time.</h2>
        <p className={styles.lede}>Save a command that worked as a Recipe, change only what varies, and run it on the machine you choose.</p>
        <button type="button" className={styles.primary} onClick={() => navigate("/login")}>
          Sign in
        </button>
      </section>
    </div>
  );
}
