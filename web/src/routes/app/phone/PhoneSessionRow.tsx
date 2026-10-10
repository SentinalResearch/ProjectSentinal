import { Link } from "react-router-dom";

import { StateTag } from "../../../components/StateTag";
import { exitUnknown } from "../../../data/capture";
import { formatSessionDuration } from "../../../data/format";
import type { Session } from "../../../data/types";
import styles from "./phoneSession.module.css";

/**
 * Phone Session row (MOBILE_SPEC.md → Session row): state badge and duration,
 * then the title, then the Command. The badge never shares the title's line.
 */
export function PhoneSessionRow({ session }: { session: Session }) {
  return (
    <Link to={`/app/sessions/${session.id}`} className={styles.row}>
      <span className={styles.rowStatus}>
        <StateTag state={session.state} size="sm" exitUnknown={exitUnknown(session)} />
        <span className="mono">{formatSessionDuration(session)}</span>
      </span>
      <span className={styles.rowTitle}>{session.name}</span>
      <span className={`${styles.rowCommand} mono`}>{session.command ?? session.cwd ?? "—"}</span>
    </Link>
  );
}
