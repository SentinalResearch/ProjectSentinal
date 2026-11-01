import { answerChoice, offeredChoices } from "../data/actions";
import type { Session } from "../data/types";
import styles from "./PromptChoices.module.css";

/**
 * The answers a waiting program offered (6.1), e.g. Claude Code's
 * "1. Yes / 2. Yes, and don't ask again … / 3. No …". Each button types that
 * option's own key; labels are terminal text, shown but never trusted.
 */
export function PromptChoices({ session, onAnswered }: { session: Session; onAnswered?: () => void }) {
  const choices = offeredChoices(session);
  if (choices.length === 0) return null;
  return (
    <div className={styles.list} role="group" aria-label={`Answers for ${session.name}`}>
      {choices.map((choice, index) => {
        const no = /^(no|deny|cancel)\b/i.test(choice.label);
        return (
          <button
            key={`${choice.keys}-${index}`}
            type="button"
            className={index === 0 ? styles.primary : no ? styles.decline : styles.choice}
            title={choice.label}
            aria-label={`Answer ${session.name}: ${choice.label}`}
            onClick={(event) => {
              event.stopPropagation();
              void answerChoice(session.id, choice).then(onAnswered, () => undefined);
            }}
          >
            <span className={styles.key} aria-hidden="true">
              {choice.keys === "\u001b" ? "esc" : choice.keys.replace("\r", "")}
            </span>
            <span className={styles.label}>{choice.label}</span>
          </button>
        );
      })}
    </div>
  );
}
