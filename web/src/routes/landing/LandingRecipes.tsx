import { useRef, type RefObject } from "react";

import { useReveal } from "../../hooks/useReveal";
import { useSequentialTyping } from "../../hooks/useTypedText";
import styles from "./LandingRecipes.module.css";

const T1 = "vllm serve {model} --port {port} --max-model-len {context}";
const T2 = "vllm serve Qwen/Qwen3-8B --port 8000 --max-model-len 32768";
const T3 = "Serving on :8000";

export interface LandingRecipesProps {
  scrollContainerRef: RefObject<HTMLDivElement | null>;
}

/** design_handoff_sentinal/design/SentinalLanding.dc.html — recipes/terminal section (§5), 28ms/char typing once revealed. */
export function LandingRecipes({ scrollContainerRef }: LandingRecipesProps) {
  const ref = useRef<HTMLDivElement | null>(null);
  const seen = useReveal(ref, scrollContainerRef, 0.7);

  const { revealed } = useSequentialTyping(seen ? [`# ${T1}`, `RTX-PC $ ${T2}`, T3] : ["", "", ""], 28, seen);

  return (
    <section ref={ref} className={`${styles.section} ${seen ? styles.seen : ""}`}>
      <div className={styles.inner}>
        <div className={styles.copy}>
          <span className={styles.overline}>recipes</span>
          <span className={styles.heading}>Run it the same way, every time.</span>
          <span className={styles.desc}>
            Save a command once with its parameters. Pick a machine, check the rendered command, run.
          </span>
        </div>
        <div className={styles.termWrap}>
          <div className={styles.termHead}>
            <span className={styles.termDot} />
            RTX-PC · Qwen vLLM Server
          </div>
          <div className={styles.termBody}>
            <div className={styles.termComment}>{revealed[0]}</div>
            <div>{revealed[1]}</div>
            <div className={styles.termOutput}>
              {revealed[2]}
              <span className={styles.cursor} aria-hidden="true" />
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
