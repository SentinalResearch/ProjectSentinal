import { Link, useLocation, useNavigate } from "react-router-dom";

import { SentinalMark } from "../../../components/SentinalMark";
import { useLiveData } from "../../../data/LiveDataContext";
import { islandLocation } from "../shell/location";
import { needCount, tickerItems } from "../shell/ticker";
import type { IslandBack } from "./islandBack";
import styles from "./MobileIsland.module.css";

/**
 * The phone island (MOBILE_GLASS_SPEC §1): logo · page · the one most urgent
 * live item · the needs-you count. On a detail screen: ‹ back, the title, and
 * the screen's own trailing control. Over the terminal it is `docked`: a
 * flat bar above the terminal instead of glass floating on it.
 */
export function MobileIsland({ back, docked = false }: { back: IslandBack | null; docked?: boolean }) {
  const navigate = useNavigate();
  const { pathname, search, key } = useLocation();
  const { machines, sessions, terminals } = useLiveData();
  const where = islandLocation(pathname, search, machines, sessions);
  const [top] = tickerItems(terminals, 1);
  const needs = needCount(terminals);

  if (back) {
    const session = where.kind === "session" ? sessions.find((entry) => entry.id === where.sessionId) : undefined;
    const title = where.kind === "session" ? where.title : where.name;
    const subtitle = where.kind === "session" ? [where.machine, session?.cwd].filter(Boolean).join(" · ") : null;
    const goBack = () => (key !== "default" ? navigate(-1) : navigate(back.parentPath, { replace: true }));
    return (
      <nav className={styles.island} aria-label="Sentinal" data-docked={docked || undefined}>
        <button type="button" className={styles.back} onClick={goBack} aria-label={`Back to ${back.parentLabel}`}>
          <span aria-hidden="true">‹</span>
        </button>
        <span className={styles.titles}>
          <span className={styles.title}>{title}</span>
          {subtitle && <span className={styles.subtitle}>{subtitle}</span>}
        </span>
        {back.trailing}
      </nav>
    );
  }

  const name = pathname.startsWith("/app/split") ? "Terminals" : where.kind === "page" ? where.name : where.title;
  return (
    <nav className={styles.island} aria-label="Sentinal" data-docked={docked || undefined}>
      <Link to="/app/overview" className={styles.logo} aria-label="Dashboard">
        <SentinalMark size={18} />
      </Link>
      <span className={styles.page}>{name}</span>
      {top && (
        <Link to={`/app/sessions/${encodeURIComponent(top.sessionId)}`} className={styles.ticker} data-state={top.state}>
          <span className={styles.square} aria-hidden="true" />
          <span className={styles.tickerText}>{top.text}</span>
        </Link>
      )}
      {needs > 0 && (
        <Link to="/app/overview?needs=1" className={styles.needs} aria-label={`${needs} need you`}>
          {needs}
        </Link>
      )}
    </nav>
  );
}
